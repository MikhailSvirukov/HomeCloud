//! Integration tests exercising the public TLS metrics listener over real
//! loopback sockets, covering `NFR-OBS-007 / 1` and `CFG-OBS-004 / 1`: a
//! scraper that trusts the configured certificate receives the same bounded
//! exposition over TLS, while an untrusted certificate, a plaintext client, and
//! invalid certificate material are rejected.

use core::num::NonZeroUsize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rcgen::CertifiedKey;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use storage_node::{Metrics, MetricsServer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_rustls::TlsConnector;

const EXPOSITION_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const SERVER_NAME: &str = "localhost";
const REQUEST: &str = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

fn queue_capacity() -> NonZeroUsize {
    NonZeroUsize::new(16).expect("test capacity is non-zero")
}

fn loopback_ephemeral_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

fn server_name() -> ServerName<'static> {
    ServerName::try_from(SERVER_NAME).expect("the test server name is a valid DNS name")
}

fn self_signed_certificate() -> CertifiedKey {
    rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_owned()])
        .expect("a self-signed localhost certificate can be generated")
}

/// A TLS metrics server bound to an ephemeral loopback port, with the
/// background task that serves scrapes until the test aborts it.
struct TestServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start(certificate: &CertifiedKey) -> Self {
        let metrics = Arc::new(Metrics::new(queue_capacity()));
        let server = MetricsServer::bind_tls(
            loopback_ephemeral_addr(),
            metrics,
            certificate.cert.pem().as_bytes(),
            certificate.key_pair.serialize_pem().as_bytes(),
        )
        .await
        .expect("binding an ephemeral loopback TLS metrics port cannot fail");
        let addr = server
            .local_addr()
            .expect("a bound metrics server reports its address");
        let task = tokio::spawn(async move {
            server
                .serve()
                .await
                .expect("the metrics server serves until the task is aborted");
        });
        Self { addr, task }
    }

    fn abort(self) {
        self.task.abort();
    }
}

/// Builds a client configuration that trusts only `certificate`.
fn trusted_client_config(certificate: &CertifiedKey) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots
        .add(certificate.cert.der().clone())
        .expect("the test certificate is a root");
    client_config(roots)
}

/// Builds a client configuration with an empty root store.
fn untrusted_client_config() -> ClientConfig {
    client_config(RootCertStore::empty())
}

fn client_config(roots: RootCertStore) -> ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the safe default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

#[tokio::test]
async fn scrapes_metrics_over_tls_with_a_trusted_certificate() {
    let certificate = self_signed_certificate();
    let server = TestServer::start(&certificate).await;
    let connector = TlsConnector::from(Arc::new(trusted_client_config(&certificate)));

    let tcp = TcpStream::connect(server.addr)
        .await
        .expect("connecting to the bound metrics address cannot fail");
    let mut stream = connector
        .connect(server_name(), tcp)
        .await
        .expect("the scraper trusts the configured certificate");
    stream
        .write_all(REQUEST.as_bytes())
        .await
        .expect("writing the HTTPS request cannot fail");
    stream
        .flush()
        .await
        .expect("flushing the HTTPS request cannot fail");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("reading the HTTPS response to EOF cannot fail");

    let response = String::from_utf8(response).expect("the metrics endpoint returns UTF-8 text");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("an HTTP response separates headers from the body");
    assert_eq!(
        head.lines().next(),
        Some("HTTP/1.1 200 OK"),
        "a trusted TLS scrape returns the exposition"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("content-type: {EXPOSITION_CONTENT_TYPE}")),
        "the TLS response keeps the Prometheus text content type"
    );
    assert!(
        body.contains("homecloud_node_up 1"),
        "the TLS response carries the same bounded exposition"
    );

    server.abort();
}

#[tokio::test]
async fn rejects_an_untrusted_certificate() {
    let certificate = self_signed_certificate();
    let server = TestServer::start(&certificate).await;
    let connector = TlsConnector::from(Arc::new(untrusted_client_config()));

    let tcp = TcpStream::connect(server.addr)
        .await
        .expect("connecting to the bound metrics address cannot fail");
    let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, connector.connect(server_name(), tcp))
        .await
        .expect("the untrusted handshake terminates before the timeout");

    assert!(
        handshake.is_err(),
        "a scraper without the configured trust material must reject the certificate"
    );

    server.abort();
}

#[tokio::test]
async fn rejects_a_plaintext_client_on_the_tls_port() {
    let certificate = self_signed_certificate();
    let server = TestServer::start(&certificate).await;
    let mut stream = TcpStream::connect(server.addr)
        .await
        .expect("connecting to the bound metrics address cannot fail");

    stream
        .write_all(REQUEST.as_bytes())
        .await
        .expect("writing the plaintext request cannot fail");
    stream
        .flush()
        .await
        .expect("flushing the plaintext request cannot fail");
    let mut buffer = [0_u8; 512];
    let read = tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.read(&mut buffer))
        .await
        .map(|result| result.unwrap_or_default())
        .unwrap_or_default();
    let response = &buffer[..read];
    assert!(
        !response
            .windows(b"HTTP/1.1 200".len())
            .any(|window| window == b"HTTP/1.1 200"),
        "a plaintext client must never receive the exposition status line"
    );
    assert!(
        !response
            .windows(b"homecloud_node_up".len())
            .any(|window| window == b"homecloud_node_up"),
        "a plaintext client must never receive the exposition body"
    );

    server.abort();
}

#[tokio::test]
async fn rejects_invalid_certificate_material() {
    let certificate = self_signed_certificate();
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();
    let metrics = Arc::new(Metrics::new(queue_capacity()));
    let cases: [(Vec<u8>, Vec<u8>); 4] = [
        (b"not a certificate".to_vec(), b"not a key".to_vec()),
        (cert_pem.as_bytes().to_vec(), b"not a key".to_vec()),
        (cert_pem.as_bytes().to_vec(), Vec::new()),
        (Vec::new(), key_pem.as_bytes().to_vec()),
    ];

    for (cert, key) in cases {
        let result =
            MetricsServer::bind_tls(loopback_ephemeral_addr(), Arc::clone(&metrics), &cert, &key)
                .await;
        assert!(
            result.is_err(),
            "bind_tls must reject certificate material it cannot parse"
        );
    }
}
