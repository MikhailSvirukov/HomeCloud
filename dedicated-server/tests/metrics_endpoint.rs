//! Integration test for the Prometheus metrics endpoint and the run loop.
//!
//! The test starts the compiled binary with a loopback metrics bind, scrapes
//! `GET /metrics` while the process runs, stops it with `SIGTERM` and checks
//! the shutdown record. The exposition itself is unit-tested in `metrics.rs`;
//! this file proves the endpoint, the configuration and the run loop.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use dedicated_server::CONFIG_PATH_ENV;

/// Binary under test, provided by Cargo for integration tests.
const BINARY: &str = env!("CARGO_BIN_EXE_dedicated-server");
/// Environment variable the binary reads its log filter from.
const RUST_LOG_ENV: &str = "RUST_LOG";
/// Log level that keeps the startup summary visible.
const RUST_LOG_LEVEL: &str = "info";
/// Configuration template with placeholders substituted per test.
const CONFIG_TEMPLATE: &str = include_str!("fixtures/server_config.toml.tmpl");
const ENCRYPTION_KEY_PLACEHOLDER: &str = "{{ENCRYPTION_KEY}}";
const SIGNING_KEY_PLACEHOLDER: &str = "{{SIGNING_KEY}}";
const LABEL_PLACEHOLDER: &str = "{{LABEL}}";
const LOG_PATH_PLACEHOLDER: &str = "{{LOG_PATH}}";
const METRICS_BIND_PLACEHOLDER: &str = "{{METRICS_BIND}}";
const ENCRYPTION_KEY: &[u8] = b"encryption-key-material-0123456789";
const SIGNING_KEY: &[u8] = b"signing-key-material-abcdefghijklmnopqrstuvwxyz";
const LABEL: &str = "dedicated-1";
/// Upper bound for starting the server and for a scrape.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Owns a per-test temporary directory and removes it on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dedicated-server-metrics-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test directory must be created");
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Reserve a loopback port and return it, releasing it for the server.
fn free_port() -> u16 {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("an ephemeral loopback port must be available");
    listener
        .local_addr()
        .expect("the local address must exist")
        .port()
}

/// Write a configuration for the given log path and metrics bind.
fn write_config(dir: &TempDir, log: &Path, metrics_bind: &str) -> PathBuf {
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");

    let config = CONFIG_TEMPLATE
        .replace(
            ENCRYPTION_KEY_PLACEHOLDER,
            &encryption.display().to_string(),
        )
        .replace(SIGNING_KEY_PLACEHOLDER, &signing.display().to_string())
        .replace(LABEL_PLACEHOLDER, LABEL)
        .replace(LOG_PATH_PLACEHOLDER, &log.display().to_string())
        .replace(METRICS_BIND_PLACEHOLDER, metrics_bind);
    let path = dir.file("dedicated-server.toml");
    std::fs::write(&path, config).expect("test configuration must be written");
    path
}

/// Spawn the binary with the configuration path in the environment.
fn spawn(config: &Path) -> Child {
    Command::new(BINARY)
        .env(CONFIG_PATH_ENV, config)
        .env(RUST_LOG_ENV, RUST_LOG_LEVEL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dedicated-server binary must start")
}

/// Send `SIGTERM` to a child process.
fn terminate(child: &Child) {
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill must run");
    assert!(status.success(), "SIGTERM must be delivered");
}

/// Scrape `GET /metrics` and return the raw response.
fn scrape(port: u16) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

/// Scrape until the server answers `200`, or fail after the timeout.
fn wait_for_metrics(port: u16) -> String {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Ok(response) = scrape(port) {
            if response.starts_with("HTTP/1.1 200 OK") {
                return response;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the metrics endpoint never answered on port {port}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Return the log events written so far.
fn log_events(path: &Path) -> Vec<Value> {
    let contents = std::fs::read_to_string(path).unwrap_or_default();
    contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("log line must be a JSON object"))
        .collect()
}

#[test]
fn the_running_server_serves_metrics_and_stops_on_sigterm() {
    let dir = TempDir::new("scrape");
    let log = dir.file("dedicated-server.log");
    let port = free_port();
    let config = write_config(&dir, &log, &format!("127.0.0.1:{port}"));

    let child = spawn(&config);
    let response = wait_for_metrics(port);

    assert!(response.contains("Content-Type: text/plain; version=0.0.4; charset=utf-8"));
    for expected in [
        "homecloud_dedicated_up 1",
        "homecloud_dedicated_node_available{node=\"storage-1\"} 1",
        "homecloud_dedicated_node_available{node=\"storage-2\"} 1",
        "homecloud_dedicated_quorum_available 1",
        "homecloud_dedicated_log_dropped_records_total 0",
        "homecloud_dedicated_log_queue_capacity_records 4096",
        "homecloud_dedicated_request_duration_seconds_count{operation=\"registration\"} 0",
        "homecloud_dedicated_request_duration_seconds_count{operation=\"topology\"} 0",
    ] {
        assert!(
            response.contains(expected),
            "missing {expected}: {response}"
        );
    }
    assert!(
        !response.contains("login") && !response.contains("upload_id"),
        "labels must not carry user-unique values: {response}"
    );

    terminate(&child);
    let output: Output = child.wait_with_output().expect("the server must exit");
    assert!(
        output.status.success(),
        "the server must exit cleanly, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = log_events(&log);
    assert_eq!(events.len(), 2, "log events: {events:?}");
    assert_eq!(events[0]["event"], "startup");
    assert_eq!(events[1]["event"], "shutdown");
}

#[test]
fn a_non_loopback_metrics_bind_is_rejected_by_the_configuration() {
    let dir = TempDir::new("non-loopback");
    let log = dir.file("dedicated-server.log");
    let config = write_config(&dir, &log, "10.255.255.1:9101");

    let output = Command::new(BINARY)
        .env(CONFIG_PATH_ENV, &config)
        .env(RUST_LOG_ENV, RUST_LOG_LEVEL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("dedicated-server binary must run");

    assert_eq!(output.status.code(), Some(1), "the start must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let event: Value = serde_json::from_str(stderr.trim_end()).expect("one JSON record");
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["error_code"], "configuration_error");
    assert!(
        !log.exists(),
        "the log file must not be opened for an invalid configuration"
    );
}
