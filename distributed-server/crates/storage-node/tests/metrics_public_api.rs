//! Integration tests exercising the public Prometheus-compatible metrics
//! endpoint over real loopback sockets, covering `NFR-OBS-005 / 3` (Raft and
//! catalog state categories), `NFR-OBS-006 / 1`, `NFR-OBS-008 / 1`, and
//! `CFG-OBS-003 / 1`: the exposition response, HTTP method/path handling,
//! scrape accounting, Raft and catalog state gauges, and bounded static labels.

use core::num::NonZeroUsize;
use std::net::SocketAddr;
use std::sync::Arc;

use storage_node::{Metrics, MetricsServer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

const METRICS_PATH: &str = "/metrics";
const EXPOSITION_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";
const STORAGE_BYTES_USED_METRIC: &str = "homecloud_storage_bytes_used";
const RAFT_LEADER_METRIC: &str = "homecloud_raft_leader";
const RAFT_TERM_METRIC: &str = "homecloud_raft_term";
const RAFT_LAST_APPLIED_METRIC: &str = "homecloud_raft_last_applied";
const RAFT_VOTERS_METRIC: &str = "homecloud_raft_voters";
const CATALOG_VERSIONS_METRIC: &str = "homecloud_catalog_versions";
const CONFIGURED_STORAGE_BYTES_USED: u64 = 1234;
const RAFT_TERM: u64 = 7;
const RAFT_LAST_APPLIED: u64 = 12;
const RAFT_VOTERS: u64 = 3;
const CATALOG_VERSIONS: u64 = 5;

fn queue_capacity() -> NonZeroUsize {
    NonZeroUsize::new(16).expect("test capacity is non-zero")
}

/// A metrics server bound to an ephemeral loopback port, with the background
/// task that serves scrapes until the test aborts it.
struct TestServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        let metrics = Arc::new(Metrics::new(queue_capacity()));
        let server = MetricsServer::bind(SocketAddr::from(([127, 0, 0, 1], 0)), metrics)
            .await
            .expect("binding an ephemeral loopback metrics port cannot fail");
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

    async fn request(&self, method: &str, path: &str) -> String {
        let mut stream = TcpStream::connect(self.addr)
            .await
            .expect("connecting to the bound metrics address cannot fail");
        let request =
            format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("writing the HTTP request cannot fail");
        stream
            .flush()
            .await
            .expect("flushing the HTTP request cannot fail");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("reading the HTTP response to EOF cannot fail");
        String::from_utf8(response).expect("the metrics endpoint returns UTF-8 text")
    }

    fn abort(self) {
        self.task.abort();
    }
}

fn split_response(response: &str) -> (&str, &str) {
    response
        .split_once("\r\n\r\n")
        .expect("an HTTP response separates headers from the body")
}

fn status_line(head: &str) -> &str {
    head.lines()
        .next()
        .expect("an HTTP response starts with a status line")
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Asserts every non-comment exposition line is a `<name>[{labels}] <value>`
/// sample whose value parses as a number.
fn assert_exposition_samples(body: &str) {
    for line in body
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (series, value) = line
            .rsplit_once(' ')
            .expect("an exposition sample is a series followed by a value");
        assert!(
            value.parse::<f64>().is_ok(),
            "sample value {value:?} is not a number"
        );
        match series.split_once('{') {
            Some((name, labels)) => assert!(
                !name.is_empty() && labels.ends_with('}'),
                "sample series {series:?} is not a name with balanced labels"
            ),
            None => assert!(!series.is_empty(), "sample series must have a metric name"),
        }
    }
}

#[tokio::test]
async fn serves_prometheus_text_on_metrics() {
    let server = TestServer::start().await;

    let response = server.request("GET", METRICS_PATH).await;
    let (head, body) = split_response(&response);

    assert_eq!(status_line(head), "HTTP/1.1 200 OK");
    assert_eq!(
        header_value(head, "content-type"),
        Some(EXPOSITION_CONTENT_TYPE)
    );
    assert!(
        body.contains("homecloud_node_up 1"),
        "the exposition reports node liveness"
    );
    assert!(
        body.contains("homecloud_connection_queue_capacity 16"),
        "the exposition reports the configured queue capacity"
    );
    assert!(
        body.contains("homecloud_node_build_info{version=\""),
        "the exposition reports a build-info version label"
    );
    assert!(
        body.contains(&format!("# TYPE {STORAGE_BYTES_USED_METRIC} gauge")),
        "the exposition types the occupied-storage gauge"
    );
    assert!(
        body.contains(&format!("{STORAGE_BYTES_USED_METRIC} 0")),
        "a fresh node reports zero occupied storage bytes"
    );
    assert_exposition_samples(body);

    server.abort();
}

#[tokio::test]
async fn returns_404_for_unknown_paths() {
    let server = TestServer::start().await;

    let response = server.request("GET", "/other").await;
    let (head, _body) = split_response(&response);

    assert!(
        status_line(head).contains(" 404 "),
        "an unknown path reports 404, got {:?}",
        status_line(head)
    );

    server.abort();
}

#[tokio::test]
async fn returns_405_for_non_get() {
    let server = TestServer::start().await;

    let response = server.request("POST", METRICS_PATH).await;
    let (head, _body) = split_response(&response);

    assert!(
        status_line(head).contains(" 405 "),
        "a non-GET method reports 405, got {:?}",
        status_line(head)
    );
    assert_eq!(header_value(head, "allow"), Some("GET"));

    server.abort();
}

#[tokio::test]
async fn counts_successful_scrapes() {
    let server = TestServer::start().await;

    let first = server.request("GET", METRICS_PATH).await;
    let (first_head, _first_body) = split_response(&first);
    assert_eq!(status_line(first_head), "HTTP/1.1 200 OK");

    let second = server.request("GET", METRICS_PATH).await;
    let (second_head, second_body) = split_response(&second);
    assert_eq!(status_line(second_head), "HTTP/1.1 200 OK");
    assert!(
        second_body.contains("homecloud_metrics_scrapes_total 2"),
        "the second scrape reports a total of two scrapes"
    );

    server.abort();
}

#[test]
fn renders_configured_storage_bytes_used() {
    let metrics = Metrics::new(queue_capacity());

    metrics.set_storage_bytes_used(CONFIGURED_STORAGE_BYTES_USED);

    let body = metrics.render();
    assert!(
        body.contains(&format!(
            "{STORAGE_BYTES_USED_METRIC} {CONFIGURED_STORAGE_BYTES_USED}\n"
        )),
        "the exposition reports the configured occupied storage bytes, got {body:?}"
    );
}

/// Asserts one metric renders as a single unlabelled gauge sample with `value`
/// and carries its own HELP and TYPE lines.
fn assert_unlabelled_gauge(body: &str, name: &str, value: u64) {
    let sample = format!("{name} {value}");
    assert!(
        body.lines().any(|line| line == sample),
        "the exposition reports the unlabelled sample {sample:?}, got {body:?}"
    );
    assert!(
        body.contains(&format!("# TYPE {name} gauge\n")),
        "the exposition types {name} as a gauge"
    );
    assert!(
        body.lines()
            .any(|line| line.starts_with(&format!("# HELP {name} "))),
        "the exposition documents {name}"
    );
}

#[test]
fn renders_raft_and_catalog_state_gauges() {
    let metrics = Metrics::new(queue_capacity());

    metrics.set_raft_state(true, RAFT_TERM, Some(RAFT_LAST_APPLIED), RAFT_VOTERS);
    metrics.set_catalog_versions(CATALOG_VERSIONS);

    let body = metrics.render();
    assert_unlabelled_gauge(&body, RAFT_LEADER_METRIC, 1);
    assert_unlabelled_gauge(&body, RAFT_TERM_METRIC, RAFT_TERM);
    assert_unlabelled_gauge(&body, RAFT_LAST_APPLIED_METRIC, RAFT_LAST_APPLIED);
    assert_unlabelled_gauge(&body, RAFT_VOTERS_METRIC, RAFT_VOTERS);
    assert_unlabelled_gauge(&body, CATALOG_VERSIONS_METRIC, CATALOG_VERSIONS);
}

#[tokio::test]
async fn uses_only_bounded_static_labels() {
    let server = TestServer::start().await;

    let response = server.request("GET", METRICS_PATH).await;
    let (_head, body) = split_response(&response);

    for forbidden in ["login=", "upload_id=", "file=", "request_id="] {
        assert!(
            !body.contains(forbidden),
            "the exposition must not label metrics with {forbidden:?}"
        );
    }

    let build_info = body
        .lines()
        .find(|line| line.starts_with("homecloud_node_build_info{"))
        .expect("the exposition reports build info");
    assert_eq!(
        build_info.matches('=').count(),
        1,
        "build info carries only the version label, got {build_info:?}"
    );

    server.abort();
}

#[tokio::test]
async fn rejects_a_non_loopback_bind_address() {
    let metrics = Arc::new(Metrics::new(queue_capacity()));
    let result = MetricsServer::bind(SocketAddr::from(([0, 0, 0, 0], 0)), metrics).await;
    assert!(
        result.is_err(),
        "non-loopback metrics binding requires TLS and must be rejected"
    );
}
