//! Minimal blocking HTTP/1.1 server for the metrics endpoint.
//!
//! Only `GET /metrics` is served. The endpoint is a separate monitoring
//! interface and does not replace or share the application TCP protocol
//! (`NFR-OBS-006`). A request is read with one bounded read under one timeout,
//! connections are handled sequentially and every response closes the
//! connection, so a slow client cannot pin a worker indefinitely and the
//! listener never accumulates threads or descriptors.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Maximum number of request bytes read from one connection.
const MAX_REQUEST_BYTES: usize = 8 * 1024;

/// Read and write timeout for a single connection.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Pause after an accept error so the listener cannot spin.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Path served by the endpoint.
pub const METRICS_PATH: &str = "/metrics";

/// Content type of an HTTP error response.
const ERROR_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

/// Handle to a running metrics server.
///
/// Dropping the handle does not stop the listener; the process owns it for its
/// whole lifetime, and the operating system closes the socket on exit.
#[derive(Debug)]
pub struct MetricsServer {
    local_addr: SocketAddr,
    _thread: JoinHandle<()>,
}

impl MetricsServer {
    /// Address the listener is bound to, with the resolved port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

/// Bind `bind` and serve `body` on `GET /metrics` from one background thread.
pub fn start(
    bind: SocketAddr,
    body: Arc<dyn Fn() -> String + Send + Sync>,
) -> io::Result<MetricsServer> {
    let listener = TcpListener::bind(bind)?;
    let local_addr = listener.local_addr()?;

    let thread = thread::Builder::new()
        .name("metrics-listener".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    // A persistent accept error (for example EMFILE) must not
                    // spin the listener thread.
                    thread::sleep(ACCEPT_ERROR_BACKOFF);
                    continue;
                };
                let _ = serve(stream, body.as_ref());
            }
        })?;

    Ok(MetricsServer {
        local_addr,
        _thread: thread,
    })
}

/// Serve one HTTP/1.1 request on `stream`.
fn serve(stream: TcpStream, body: &(dyn Fn() -> String + Send + Sync)) -> io::Result<()> {
    stream.set_read_timeout(Some(CONNECTION_TIMEOUT))?;
    stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;

    let mut request = [0_u8; MAX_REQUEST_BYTES];
    let read = (&stream).read(&mut request)?;
    let request = &request[..read];
    let request_line = request
        .split(|byte| *byte == b'\n')
        .next()
        .map(|line| {
            String::from_utf8_lossy(line)
                .trim_end_matches('\r')
                .to_owned()
        })
        .unwrap_or_default();

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();

    if method.is_empty() || target.is_empty() {
        return write_response(stream, Response::error(400, "incomplete request"));
    }

    let response = match (method, path) {
        ("GET", METRICS_PATH) => Response::ok(body()),
        ("GET", _) => Response::error(404, "not found"),
        (_, METRICS_PATH) => Response::error(405, "only GET is supported"),
        _ => Response::error(404, "not found"),
    };

    write_response(stream, response)
}

fn write_response(mut stream: TcpStream, response: Response) -> io::Result<()> {
    let bytes = response.as_bytes();
    stream.write_all(&bytes)?;
    stream.flush()
}

/// One HTTP response with a small text body.
struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: String,
}

impl Response {
    fn ok(body: String) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: crate::metrics::CONTENT_TYPE,
            body,
        }
    }

    fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            reason: match status {
                404 => "Not Found",
                405 => "Method Not Allowed",
                _ => "Bad Request",
            },
            content_type: ERROR_CONTENT_TYPE,
            body: format!("{message}\n"),
        }
    }

    fn as_bytes(&self) -> Vec<u8> {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            self.reason,
            self.content_type,
            self.body.len()
        );
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(self.body.as_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;

    use super::*;

    fn body() -> String {
        "# TYPE homecloud_dedicated_up gauge\nhomecloud_dedicated_up 1\n".to_owned()
    }

    fn server() -> MetricsServer {
        let body = Arc::new(body);
        start("127.0.0.1:0".parse().expect("valid address"), body).expect("the server must bind")
    }

    fn request(server: &MetricsServer, request: &str) -> String {
        let mut stream =
            TcpStream::connect(server.local_addr()).expect("the server must accept connections");
        stream
            .write_all(request.as_bytes())
            .expect("the request must be written");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("the request must be finished");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("the response must be read");
        response
    }

    #[test]
    fn a_get_metrics_request_returns_the_exposition() {
        let server = server();
        let response = request(&server, "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains(&format!(
            "Content-Type: {}\r\n",
            crate::metrics::CONTENT_TYPE
        )));
        assert!(response.ends_with(&*body()));
    }

    #[test]
    fn a_query_string_is_ignored() {
        let server = server();
        let response = request(&server, "GET /metrics?x=1 HTTP/1.1\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn another_path_is_not_found() {
        let server = server();
        let response = request(&server, "GET / HTTP/1.1\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(response.ends_with("not found\n"));
    }

    #[test]
    fn another_method_is_not_allowed() {
        let server = server();
        let response = request(&server, "POST /metrics HTTP/1.1\r\n\r\n");

        assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
        assert!(response.ends_with("only GET is supported\n"));
    }

    #[test]
    fn an_incomplete_request_line_is_rejected() {
        let server = server();
        let response = request(&server, "GET");

        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    }
}
