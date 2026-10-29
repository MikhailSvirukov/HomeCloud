//! Integration tests for the Prometheus metrics endpoint and the run loop.
//!
//! The tests start the compiled binary with a loopback metrics bind, scrape
//! `GET /metrics` while the process runs, stop it with `SIGTERM` and check the
//! shutdown record. The exposition content is unit-tested in `metrics.rs`; this
//! file proves the endpoint, the configuration, the run loop and the failure
//! paths.

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
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
/// Server name in the generated test certificate.
const TLS_SERVER_NAME: &str = "localhost";
/// Upper bound for starting the server, a scrape and a clean exit.
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

/// A running server process that is killed and reaped if a test panics.
struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn spawn(config: &Path) -> Self {
        let mut command = Command::new(BINARY);
        command
            .env(CONFIG_PATH_ENV, config)
            .env(RUST_LOG_ENV, RUST_LOG_LEVEL);
        Self::spawn_command(&mut command)
    }

    fn spawn_command(command: &mut Command) -> Self {
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("dedicated-server binary must start");
        Self { child: Some(child) }
    }

    fn child(&mut self) -> &mut Child {
        self.child.as_mut().expect("the child must be alive")
    }

    /// Run the process to completion, killing it if it does not exit in time.
    fn wait(mut self) -> Output {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let exited = self
                .child()
                .try_wait()
                .expect("the child status must be readable")
                .is_some();
            if exited {
                break;
            }
            if Instant::now() >= deadline {
                let _ = self.child().kill();
                let _ = self.child().wait();
                panic!("the server did not exit within {TIMEOUT:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        self.child
            .take()
            .expect("the child must be alive")
            .wait_with_output()
            .expect("the server output must be collected")
    }

    /// Send `SIGTERM`, wait at most [`TIMEOUT`] for the process and collect it.
    fn terminate(mut self) -> Output {
        let pid = self.child().id().to_string();
        let signal = Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .expect("kill must run");
        assert!(signal.success(), "SIGTERM must be delivered");

        let deadline = Instant::now() + TIMEOUT;
        loop {
            let exited = self
                .child()
                .try_wait()
                .expect("the child status must be readable")
                .is_some();
            if exited {
                break;
            }
            if Instant::now() >= deadline {
                let _ = self.child().kill();
                let _ = self.child().wait();
                panic!("the server did not stop within {TIMEOUT:?} after SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        self.child
            .take()
            .expect("the child must be alive")
            .wait_with_output()
            .expect("the server output must be collected")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
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

/// Reserve a loopback port for the metrics endpoint.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral loopback port must be available")
        .local_addr()
        .expect("the local address must exist")
        .port()
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

/// Scrape until the server answers `200`, failing fast if the child exits.
fn wait_for_metrics(guard: &mut ChildGuard, port: u16) -> String {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Ok(response) = scrape(port) {
            if response.starts_with("HTTP/1.1 200 OK") {
                return response;
            }
        }
        if let Some(status) = guard
            .child()
            .try_wait()
            .expect("the child status must be readable")
        {
            panic!("the server exited before the endpoint answered: {status}");
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

fn is_metric_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic() || first == '_' || first == ':')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == ':'
        })
}

fn is_label_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Assert that the body is a parseable Prometheus text exposition.
fn assert_parseable_exposition(body: &str) {
    for line in body.lines().filter(|line| !line.is_empty()) {
        if line.starts_with('#') {
            continue;
        }
        let (series, value) = line
            .rsplit_once(' ')
            .unwrap_or_else(|| panic!("metric line must carry a value: {line}"));
        value
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("metric value must be a number: {line}"));

        let name = series.split('{').next().unwrap_or_default();
        assert!(is_metric_name(name), "invalid metric name in: {line}");

        if let Some(labels) = series.strip_prefix(name) {
            if !labels.is_empty() {
                let labels = labels
                    .strip_prefix('{')
                    .and_then(|labels| labels.strip_suffix('}'))
                    .unwrap_or_else(|| panic!("invalid label set in: {line}"));
                for pair in labels.split(',') {
                    let (key, value) = pair
                        .split_once('=')
                        .unwrap_or_else(|| panic!("invalid label in: {line}"));
                    assert!(is_label_name(key), "invalid label name in: {line}");
                    assert!(
                        value.starts_with('"') && value.ends_with('"') && value.len() >= 2,
                        "label value must be quoted in: {line}"
                    );
                }
            }
        }
    }

    assert!(
        body.contains("le=\"+Inf\""),
        "the histogram must render its +Inf bucket"
    );
    assert!(
        body.contains("_sum{") && body.contains("_count{"),
        "the histogram must render its sum and count"
    );
}

#[test]
fn the_running_server_serves_metrics_and_stops_on_sigterm() {
    let dir = TempDir::new("scrape");
    let log = dir.file("dedicated-server.log");
    let port = free_port();
    let config = write_config(&dir, &log, &format!("127.0.0.1:{port}"));

    let mut guard = ChildGuard::spawn(&config);
    let response = wait_for_metrics(&mut guard, port);

    assert!(response.contains("Content-Type: text/plain; version=0.0.4; charset=utf-8"));
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("the response must carry a body");
    assert_parseable_exposition(body);
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

    let output = guard.terminate();
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
fn an_occupied_metrics_port_is_a_metrics_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("the port must be reserved");
    let port = listener
        .local_addr()
        .expect("the local address must exist")
        .port();

    let dir = TempDir::new("occupied");
    let log = dir.file("dedicated-server.log");
    let config = write_config(&dir, &log, &format!("127.0.0.1:{port}"));

    let output = ChildGuard::spawn(&config).wait();

    assert_eq!(output.status.code(), Some(1), "the start must fail");
    let events = log_events(&log);
    let failure = events.last().expect("a failure record must be written");
    assert_eq!(failure["event"], "startup");
    assert_eq!(failure["outcome"], "failure");
    assert_eq!(failure["error_code"], "metrics_error");
}

#[test]
fn a_non_loopback_metrics_bind_is_rejected_by_the_configuration() {
    let dir = TempDir::new("non-loopback");
    let log = dir.file("dedicated-server.log");
    let config = write_config(&dir, &log, "10.255.255.1:9101");

    let output = ChildGuard::spawn(&config).wait();

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

/// Test certificate material: the server leaf, its key and the CA trust file.
struct TlsMaterial {
    certificate: PathBuf,
    private_key: PathBuf,
    trust: PathBuf,
}

/// Generate a CA and a server certificate for `localhost` signed by it.
///
/// The CA is the trust material a collecting client uses; a self-signed
/// end-entity certificate is not a valid trust anchor for webpki.
fn write_tls_material(dir: &TempDir, name: &str) -> TlsMaterial {
    let mut ca_params = rcgen::CertificateParams::new(Vec::new()).expect("CA parameters");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.distinguished_name.push(
        rcgen::DnType::CommonName,
        format!("HomeCloud test CA {name}"),
    );
    let ca_key = rcgen::KeyPair::generate().expect("the CA key must be generated");
    let ca_certificate = ca_params
        .self_signed(&ca_key)
        .expect("the CA certificate must be signed");

    let mut server_params =
        rcgen::CertificateParams::new(vec![TLS_SERVER_NAME.to_owned()]).expect("server parameters");
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, TLS_SERVER_NAME);
    server_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let server_key = rcgen::KeyPair::generate().expect("the server key must be generated");
    let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
    let server_certificate = server_params
        .signed_by(&server_key, &issuer)
        .expect("the server certificate must be signed");

    let certificate = dir.file(&format!("{name}-server.crt"));
    let private_key = dir.file(&format!("{name}-server.key"));
    let trust = dir.file(&format!("{name}-ca.crt"));
    std::fs::write(&certificate, server_certificate.pem()).expect("certificate must be written");
    std::fs::write(&private_key, server_key.serialize_pem()).expect("key must be written");
    std::fs::write(&trust, ca_certificate.pem()).expect("trust material must be written");

    TlsMaterial {
        certificate,
        private_key,
        trust,
    }
}

/// Append the TLS section to a written configuration document.
fn append_tls_section(
    config: &Path,
    certificate: &Path,
    private_key: &Path,
    trust: &Path,
) -> PathBuf {
    let mut contents = std::fs::read_to_string(config).expect("the configuration must be readable");
    contents.push_str(&format!(
        "\n[metrics.tls]\ncertificate_path = \"{}\"\nprivate_key_path = \"{}\"\ntrust_path = \"{}\"\n",
        certificate.display(),
        private_key.display(),
        trust.display()
    ));
    std::fs::write(config, contents).expect("the configuration must be written");
    config.to_path_buf()
}

/// Scrape over TLS, verifying the server against `trust`.
fn tls_scrape(port: u16, trust: &Path) -> std::io::Result<String> {
    let mut roots = RootCertStore::empty();
    let mut reader = BufReader::new(std::fs::File::open(trust)?);
    for certificate in rustls_pemfile::certs(&mut reader) {
        roots
            .add(certificate.map_err(|error| std::io::Error::other(error.to_string()))?)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
    }

    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
    let name = ServerName::try_from(TLS_SERVER_NAME)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let connection = ClientConnection::new(Arc::new(config), name)
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    let stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut stream = StreamOwned::new(connection, stream);
    stream.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

/// Scrape over TLS until the server answers `200`, failing fast if it exits.
fn wait_for_tls_metrics(guard: &mut ChildGuard, port: u16, trust: &Path) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut last_error = String::from("no attempt yet");
    loop {
        match tls_scrape(port, trust) {
            Ok(response) if response.starts_with("HTTP/1.1 200 OK") => return response,
            Ok(response) => last_error = format!("unexpected response: {response}"),
            Err(error) => last_error = error.to_string(),
        }
        if let Some(status) = guard
            .child()
            .try_wait()
            .expect("the child status must be readable")
        {
            panic!("the server exited before the TLS endpoint answered: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "the TLS metrics endpoint never answered on port {port}; last error: {last_error}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_tls_scrape_with_the_trusted_certificate_succeeds() {
    let dir = TempDir::new("tls");
    let log = dir.file("dedicated-server.log");
    let material = write_tls_material(&dir, "trusted");
    let port = free_port();
    let config = write_config(&dir, &log, &format!("127.0.0.1:{port}"));
    let config = append_tls_section(
        &config,
        &material.certificate,
        &material.private_key,
        &material.trust,
    );

    let mut guard = ChildGuard::spawn(&config);
    let response = wait_for_tls_metrics(&mut guard, port, &material.trust);

    assert!(response.contains("homecloud_dedicated_up 1"), "{response}");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("the response must carry a body");
    assert_parseable_exposition(body);

    let output = guard.terminate();
    assert!(output.status.success(), "the server must exit cleanly");
    let events = log_events(&log);
    assert_eq!(events.len(), 2, "log events: {events:?}");
    assert_eq!(events[1]["event"], "shutdown");
}

#[test]
fn an_untrusted_certificate_is_rejected() {
    let dir = TempDir::new("tls-untrusted");
    let log = dir.file("dedicated-server.log");
    let material = write_tls_material(&dir, "server");
    let port = free_port();
    let config = write_config(&dir, &log, &format!("127.0.0.1:{port}"));
    let config = append_tls_section(
        &config,
        &material.certificate,
        &material.private_key,
        &material.trust,
    );

    // A separate CA is what the collecting client trusts, so the handshake fails.
    let other = write_tls_material(&dir, "untrusted");

    let mut guard = ChildGuard::spawn(&config);
    // Wait until the endpoint is up by scraping with the correct trust store.
    wait_for_tls_metrics(&mut guard, port, &material.trust);
    assert!(
        tls_scrape(port, &other.trust).is_err(),
        "a client that does not trust the server certificate must fail the handshake"
    );

    let output = guard.terminate();
    assert!(output.status.success(), "the server must exit cleanly");
}
