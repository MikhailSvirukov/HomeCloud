//! Prometheus-compatible metrics endpoint.
//!
//! SRS: `NFR-OBS-005 / 3`, `NFR-OBS-006 / 1`, `NFR-OBS-007 / 1`, `NFR-OBS-008 / 1`,
//! `CFG-OBS-003 / 1`, `CFG-OBS-004 / 1`.

use core::num::NonZeroUsize;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// Maximum number of request bytes read before a response is written.
const MAX_REQUEST_LEN: usize = 8 * 1024;
/// Request read size per socket operation.
const READ_CHUNK_LEN: usize = 1024;
/// Terminator that ends the request headers.
const REQUEST_TERMINATOR: &[u8] = b"\r\n\r\n";
/// Only path served with a metrics response.
const METRICS_PATH: &str = "/metrics";
/// Prometheus text exposition content type.
const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Runtime metrics rendered by the monitoring endpoint.
pub struct Metrics {
    queue_capacity: NonZeroUsize,
    scrapes: AtomicU64,
    storage_bytes_used: AtomicU64,
}

impl Metrics {
    /// Builds the metric set for a node with the configured queue capacity.
    pub fn new(queue_capacity: NonZeroUsize) -> Self {
        Self {
            queue_capacity,
            scrapes: AtomicU64::new(0),
            storage_bytes_used: AtomicU64::new(0),
        }
    }

    /// Sets the local storage bytes used by the accounts and raft roots.
    pub fn set_storage_bytes_used(&self, bytes: u64) {
        self.storage_bytes_used.store(bytes, Ordering::Relaxed);
    }

    /// Renders the Prometheus text exposition; no user-controlled label values.
    pub fn render(&self) -> String {
        let scrapes = self.scrapes.load(Ordering::Relaxed);
        let storage_bytes_used = self.storage_bytes_used.load(Ordering::Relaxed);
        format!(
            "# HELP homecloud_node_up Whether the storage node is running.\n\
             # TYPE homecloud_node_up gauge\n\
             homecloud_node_up 1\n\
             # HELP homecloud_storage_bytes_used Local bytes used under the accounts and raft roots.\n\
             # TYPE homecloud_storage_bytes_used gauge\n\
             homecloud_storage_bytes_used {storage_bytes_used}\n\
             # HELP homecloud_connection_queue_capacity Configured per-connection queue capacity.\n\
             # TYPE homecloud_connection_queue_capacity gauge\n\
             homecloud_connection_queue_capacity {}\n\
             # HELP homecloud_metrics_scrapes_total Successful metrics scrapes.\n\
             # TYPE homecloud_metrics_scrapes_total counter\n\
             homecloud_metrics_scrapes_total {}\n\
             # HELP homecloud_node_build_info Build information for the storage node.\n\
             # TYPE homecloud_node_build_info gauge\n\
             homecloud_node_build_info{{version=\"{}\"}} 1\n",
            self.queue_capacity.get(),
            scrapes,
            env!("CARGO_PKG_VERSION"),
        )
    }

    /// Counts one successful scrape before its body is rendered.
    fn record_scrape(&self) {
        self.scrapes.fetch_add(1, Ordering::Relaxed);
    }
}

impl core::fmt::Debug for Metrics {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Metrics")
            .field("queue_capacity", &self.queue_capacity)
            .field("scrapes", &self.scrapes.load(Ordering::Relaxed))
            .field(
                "storage_bytes_used",
                &self.storage_bytes_used.load(Ordering::Relaxed),
            )
            .finish()
    }
}

/// Configured certificate and private-key paths for the TLS metrics endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsTlsConfig {
    /// Path of the PEM-encoded server certificate chain.
    pub cert_path: PathBuf,
    /// Path of the PEM-encoded matching private key.
    pub key_path: PathBuf,
}

/// Bound monitoring listener that answers Prometheus scrapes.
pub struct MetricsServer {
    listener: TcpListener,
    metrics: Arc<Metrics>,
    tls: Option<TlsAcceptor>,
}

impl MetricsServer {
    /// Binds a plaintext monitoring listener to a loopback `bind_addr`.
    pub async fn bind(bind_addr: SocketAddr, metrics: Arc<Metrics>) -> io::Result<Self> {
        if !bind_addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "non-loopback metrics bind requires TLS",
            ));
        }
        let listener = TcpListener::bind(bind_addr).await?;
        Ok(Self {
            listener,
            metrics,
            tls: None,
        })
    }

    /// Binds a TLS monitoring listener to `bind_addr` with the configured PEM material.
    pub async fn bind_tls(
        bind_addr: SocketAddr,
        metrics: Arc<Metrics>,
        cert_pem: &[u8],
        key_pem: &[u8],
    ) -> io::Result<Self> {
        let tls = build_tls_acceptor(cert_pem, key_pem)?;
        let listener = TcpListener::bind(bind_addr).await?;
        Ok(Self {
            listener,
            metrics,
            tls: Some(tls),
        })
    }

    /// Returns the resolved monitoring listener address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts and answers connections until cancelled by the caller.
    pub async fn serve(self) -> io::Result<()> {
        loop {
            let (stream, _peer) = self.listener.accept().await?;
            let metrics = Arc::clone(&self.metrics);
            let tls = self.tls.clone();
            tokio::spawn(async move {
                let result = match tls {
                    Some(acceptor) => match acceptor.accept(stream).await {
                        Ok(stream) => handle_connection(stream, metrics).await,
                        Err(error) => {
                            tracing::debug!(error = %error, "metrics TLS handshake failed");
                            return;
                        }
                    },
                    None => handle_connection(stream, metrics).await,
                };
                if let Err(error) = result {
                    tracing::debug!(error = %error, "metrics connection failed");
                }
            });
        }
    }
}

/// Builds the rustls acceptor from PEM certificate and private-key bytes.
fn build_tls_acceptor(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<TlsAcceptor> {
    let mut cert_reader = cert_pem;
    let certificates = rustls_pemfile::certs(&mut cert_reader)
        .collect::<io::Result<Vec<_>>>()
        .map_err(|error| invalid_data("invalid metrics TLS certificate PEM", error))?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "metrics TLS certificate PEM contains no certificates",
        ));
    }
    let mut key_reader = key_pem;
    let private_key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|error| invalid_data("invalid metrics TLS private-key PEM", error))?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "metrics TLS private-key PEM contains no private key",
            )
        })?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|builder| {
            builder
                .with_no_client_auth()
                .with_single_cert(certificates, private_key)
        })
        .map_err(|error| invalid_data("invalid metrics TLS configuration", error))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Wraps `error` as invalid input carrying `context`.
fn invalid_data(context: &str, error: impl core::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{context}: {error}"))
}

/// Route selected from the HTTP request line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Metrics,
    NotFound,
    MethodNotAllowed,
}

/// HTTP response status supported by the monitoring endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    RequestHeaderFieldsTooLarge,
}

impl Status {
    fn code(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::RequestHeaderFieldsTooLarge => 431,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
        }
    }

    fn extra_headers(self) -> &'static str {
        match self {
            Self::MethodNotAllowed => "Allow: GET\r\n",
            _ => "",
        }
    }
}

/// Reads one request and writes exactly one response.
async fn handle_connection<S>(mut stream: S, metrics: Arc<Metrics>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK_LEN];

    let header_end = loop {
        if buffer.len() >= MAX_REQUEST_LEN {
            return respond(&mut stream, Status::RequestHeaderFieldsTooLarge, "").await;
        }
        let remaining = MAX_REQUEST_LEN - buffer.len();
        let read = stream
            .read(&mut chunk[..remaining.min(READ_CHUNK_LEN)])
            .await?;
        if read == 0 {
            return respond(&mut stream, Status::BadRequest, "").await;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_header_end(&buffer) {
            break end;
        }
    };

    let Ok(head) = core::str::from_utf8(&buffer[..header_end]) else {
        return respond(&mut stream, Status::BadRequest, "").await;
    };
    let Some(route) = parse_route(head) else {
        return respond(&mut stream, Status::BadRequest, "").await;
    };

    match route {
        Route::Metrics => {
            metrics.record_scrape();
            let body = metrics.render();
            respond(&mut stream, Status::Ok, &body).await
        }
        Route::NotFound => respond(&mut stream, Status::NotFound, "").await,
        Route::MethodNotAllowed => respond(&mut stream, Status::MethodNotAllowed, "").await,
    }
}

/// Returns the index of the header terminator, if present.
fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(REQUEST_TERMINATOR.len())
        .position(|window| window == REQUEST_TERMINATOR)
}

/// Parses the request line and selects a route, or rejects malformed input.
fn parse_route(head: &str) -> Option<Route> {
    let mut parts = head.lines().next()?.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return None;
    }
    if method != "GET" {
        return Some(Route::MethodNotAllowed);
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    if path == METRICS_PATH {
        Some(Route::Metrics)
    } else {
        Some(Route::NotFound)
    }
}

/// Writes a complete response and closes the connection.
async fn respond<S>(stream: &mut S, status: Status, body: &str) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         {}\r\n\
         {}",
        status.code(),
        status.reason(),
        CONTENT_TYPE,
        body.len(),
        status.extra_headers(),
        body,
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::parse_route;

    #[test]
    fn parse_route_rejects_malformed_request_lines() {
        assert_eq!(parse_route("GET /metrics"), None);
        assert_eq!(parse_route("GET /metrics HTTP/2.0"), None);
        assert_eq!(parse_route("GET /metrics HTTP/1.1 extra"), None);
        assert_eq!(parse_route(""), None);
    }
}
