//! Binary-level integration tests for identity-aware startup wiring.
//!
//! These tests run the compiled `dedicated-server` binary end to end and assert
//! on its exit status and output. The identity module's unit tests already cover
//! `ServerIdentity::load` in isolation, so this file drives the process instead.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

use dedicated_server::CONFIG_PATH_ENV;

/// Binary under test, provided by Cargo for integration tests.
const BINARY: &str = env!("CARGO_BIN_EXE_dedicated-server");
/// Environment variable the binary reads its log filter from.
const RUST_LOG_ENV: &str = "RUST_LOG";
/// Log level that keeps the startup summary and error events visible.
const RUST_LOG_LEVEL: &str = "info";
/// Error code the binary reports when the key identity cannot be loaded.
const IDENTITY_ERROR_CODE: &str = "identity_error";
/// Fixed message the binary reports when the key identity cannot be loaded.
const IDENTITY_FAILURE_MESSAGE: &str = "key identity loading failed";
/// Fixed message the binary reports when it stops.
const SHUTDOWN_MESSAGE: &str = "server stopped";
/// Label configured for the identity under test.
const LABEL: &str = "dedicated-1";
/// Distinct, non-empty key material for the two configured roles.
const ENCRYPTION_KEY: &[u8] = b"encryption-key-material-0123456789";
const SIGNING_KEY: &[u8] = b"signing-key-material-abcdefghijklmnopqrstuvwxyz";

/// Configuration document with placeholders substituted per test.
const CONFIG_TEMPLATE: &str = include_str!("fixtures/server_config.toml.tmpl");
const ENCRYPTION_KEY_PLACEHOLDER: &str = "{{ENCRYPTION_KEY}}";
const SIGNING_KEY_PLACEHOLDER: &str = "{{SIGNING_KEY}}";
const LABEL_PLACEHOLDER: &str = "{{LABEL}}";
const LOG_PATH_PLACEHOLDER: &str = "{{LOG_PATH}}";
/// Placeholder for the loopback metrics bind address used by the tests.
const METRICS_BIND_PLACEHOLDER: &str = "{{METRICS_BIND}}";

/// Reserve a loopback port for the metrics endpoint so the configuration is valid.
fn free_metrics_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral loopback port must be available")
        .local_addr()
        .expect("the local address must exist")
        .port()
}

/// Owns a per-test temporary directory and removes it on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dedicated-server-startup-{}-{name}",
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

/// Write a complete, valid configuration pointing at the given file paths.
fn write_config(dir: &TempDir, encryption: &Path, signing: &Path, log: &Path) -> PathBuf {
    let config = CONFIG_TEMPLATE
        .replace(
            ENCRYPTION_KEY_PLACEHOLDER,
            &encryption.display().to_string(),
        )
        .replace(SIGNING_KEY_PLACEHOLDER, &signing.display().to_string())
        .replace(LABEL_PLACEHOLDER, LABEL)
        .replace(LOG_PATH_PLACEHOLDER, &log.display().to_string())
        .replace(
            METRICS_BIND_PLACEHOLDER,
            &format!("127.0.0.1:{}", free_metrics_port()),
        );
    let path = dir.file("dedicated-server.toml");
    std::fs::write(&path, config).expect("test configuration must be written");
    path
}

/// Run the binary with the configuration path passed through the environment.
///
/// `RUST_LOG` is set explicitly so the assertions do not depend on the ambient
/// environment.
fn run_with_config(config: &Path) -> Output {
    Command::new(BINARY)
        .env(CONFIG_PATH_ENV, config)
        .env(RUST_LOG_ENV, RUST_LOG_LEVEL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("dedicated-server binary must run")
}

/// Run the binary until it logs its startup event, then stop it with `SIGTERM`.
fn run_until_started(config: &Path, log: &Path, with_log_filter: bool) -> Output {
    let mut command = Command::new(BINARY);
    command.env(CONFIG_PATH_ENV, config);
    if with_log_filter {
        command.env(RUST_LOG_ENV, RUST_LOG_LEVEL);
    } else {
        command.env_remove(RUST_LOG_ENV);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dedicated-server binary must start");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let started = std::fs::read_to_string(log)
            .is_ok_and(|contents| contents.contains("\"event\":\"startup\""));
        if started {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the server never logged its startup event"
        );
        assert!(
            child
                .try_wait()
                .expect("the child status must be readable")
                .is_none(),
            "the server exited before its startup event"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let signal = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill must run");
    assert!(signal.success(), "SIGTERM must be delivered");

    child.wait_with_output().expect("the server must exit")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Parse every non-empty stderr line and reject blank lines between records.
fn stderr_events(output: &Output) -> Vec<Value> {
    let stderr = stderr_of(output);
    let lines: Vec<&str> = stderr.lines().collect();
    assert!(
        lines.iter().all(|line| !line.is_empty()),
        "stderr must not contain blank lines: {stderr}"
    );
    lines
        .into_iter()
        .map(|line| serde_json::from_str(line).expect("stderr line must be a JSON object"))
        .collect()
}

/// Read the configured log file and parse every non-empty line.
fn configured_log(path: &Path) -> (String, Vec<Value>) {
    let contents = std::fs::read_to_string(path).expect("the configured log file must be readable");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(
        lines.iter().all(|line| !line.is_empty()),
        "the log file must not contain blank lines: {contents}"
    );
    let events = lines
        .into_iter()
        .map(|line| serde_json::from_str(line).expect("log line must be a JSON object"))
        .collect();
    (contents, events)
}

/// Assert that the log records contain no paths, file names or key material.
fn assert_no_forbidden_data(contents: &str, paths: &[&Path]) {
    for path in paths {
        let rendered = path.display().to_string();
        assert!(
            !contents.contains(&rendered),
            "log records must not contain the path {rendered}: {contents}"
        );
    }
    for name in [
        "dedicated-server.toml",
        "dedicated-server.log",
        "encryption.key",
        "signing.key",
        "absent-encryption.key",
    ] {
        assert!(
            !contents.contains(name),
            "log records must not contain the file name {name}: {contents}"
        );
    }
    for key in [ENCRYPTION_KEY, SIGNING_KEY] {
        let material = std::str::from_utf8(key).expect("test key material must be UTF-8");
        assert!(
            !contents.contains(material),
            "log records must not contain key material: {contents}"
        );
    }
}

/// Assert that a timestamp is RFC 3339 in UTC with milliseconds.
fn assert_utc_timestamp(event: &Value) {
    let timestamp = event["timestamp"]
        .as_str()
        .expect("event must carry a timestamp string");
    assert!(
        timestamp.ends_with('Z'),
        "timestamp must be UTC: {timestamp}"
    );
    assert_eq!(
        timestamp.len(),
        24,
        "timestamp must carry milliseconds: {timestamp}"
    );
}

/// Assert that an event is the shutdown record that ends every run.
fn assert_shutdown_event(event: &Value) {
    assert_eq!(event["event"], "shutdown");
    assert_eq!(event["outcome"], "success");
    assert_eq!(event["component"], "dedicated-server");
    assert_eq!(event["level"], "INFO");
    assert_utc_timestamp(event);
    assert_eq!(event["message"], SHUTDOWN_MESSAGE);
}

#[test]
fn valid_config_and_distinct_keys_log_one_summary_and_exit_zero() {
    let dir = TempDir::new("valid");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    let log = dir.file("dedicated-server.log");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing, &log);

    let output = run_until_started(&config, &log, true);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty once the log file is configured, got: {}",
        stderr_of(&output)
    );
    let (contents, events) = configured_log(&log);
    assert_eq!(events.len(), 2, "log contents: {contents}");
    let event = &events[0];
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "success");
    assert_eq!(event["component"], "dedicated-server");
    assert_eq!(event["level"], "INFO");
    assert_utc_timestamp(event);
    assert_shutdown_event(&events[1]);
    let summary = event["message"]
        .as_str()
        .expect("startup event must carry a message");
    assert!(
        summary.contains("listening on 10.0.0.1:7000"),
        "message: {summary}"
    );
    assert!(
        summary.contains(&format!("label={LABEL}")),
        "message: {summary}"
    );
    assert!(summary.contains("nodes=2"), "message: {summary}");
    assert!(summary.contains("interval_ms=1000"), "message: {summary}");
    assert!(summary.contains("timeout_ms=500"), "message: {summary}");
    assert!(summary.contains("miss_threshold=3"), "message: {summary}");
    assert!(
        summary.contains(&format!("encryption_key_bytes={}", ENCRYPTION_KEY.len())),
        "message: {summary}"
    );
    assert!(
        summary.contains(&format!("signing_key_bytes={}", SIGNING_KEY.len())),
        "message: {summary}"
    );
    assert_no_forbidden_data(&contents, &[&config, &log, &encryption, &signing]);
}

#[test]
fn invalid_configuration_writes_one_bootstrap_failure_record_to_stderr() {
    let dir = TempDir::new("invalid-configuration");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    let log = dir.file("dedicated-server.log");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing, &log);
    let invalid = std::fs::read_to_string(&config)
        .expect("test configuration must be readable")
        .replace("listen = \"10.0.0.1:7000\"", "listen = \"not-a-socket\"");
    std::fs::write(&config, invalid).expect("invalid configuration must be written");

    let output = run_with_config(&config);

    assert_eq!(
        output.status.code(),
        Some(1),
        "the process must exit with the failure code, stderr: {}",
        stderr_of(&output)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    let events = stderr_events(&output);
    assert_eq!(events.len(), 1, "stderr: {}", stderr_of(&output));
    let event = &events[0];
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["level"], "ERROR");
    assert_eq!(event["error_code"], "configuration_error");
    assert_eq!(event["message"], "configuration loading failed");
    assert!(!log.exists(), "no log file may be opened before the config");
    assert_no_forbidden_data(&stderr_of(&output), &[&config, &log, &encryption, &signing]);
}

#[test]
fn unopenable_log_path_fails_with_the_logging_error_code_on_stderr() {
    let dir = TempDir::new("unopenable-log");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    let log = dir.file("missing").join("dedicated-server.log");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing, &log);

    let output = run_with_config(&config);

    assert_eq!(
        output.status.code(),
        Some(1),
        "the process must exit with the failure code, stderr: {}",
        stderr_of(&output)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    let events = stderr_events(&output);
    assert_eq!(events.len(), 1, "stderr: {}", stderr_of(&output));
    let event = &events[0];
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["level"], "ERROR");
    assert_eq!(event["error_code"], "logging_error");
    assert_eq!(event["message"], "logging initialization failed");
    assert_no_forbidden_data(&stderr_of(&output), &[&config, &encryption, &signing]);
}

#[test]
fn missing_encryption_key_file_fails_with_the_identity_error_code_and_no_paths() {
    let dir = TempDir::new("missing-encryption");
    let missing = dir.file("absent-encryption.key");
    let signing = dir.file("signing.key");
    let log = dir.file("dedicated-server.log");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &missing, &signing, &log);

    let output = run_with_config(&config);

    assert_eq!(
        output.status.code(),
        Some(1),
        "the process must exit with the failure code, stderr: {}",
        stderr_of(&output)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty once the log file is configured, got: {}",
        stderr_of(&output)
    );
    let (contents, events) = configured_log(&log);
    assert_eq!(events.len(), 1, "log contents: {contents}");
    let event = &events[0];
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["level"], "ERROR");
    assert_eq!(event["error_code"], IDENTITY_ERROR_CODE);
    assert_eq!(event["component"], "dedicated-server");
    assert_utc_timestamp(event);
    let message = event["message"]
        .as_str()
        .expect("startup failure event must carry a message");
    assert_eq!(message, IDENTITY_FAILURE_MESSAGE);
    assert_no_forbidden_data(&contents, &[&config, &log, &missing, &signing]);
}

#[test]
fn unset_log_filter_still_logs_startup_summary_and_exits_zero() {
    let dir = TempDir::new("default-log-filter");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    let log = dir.file("dedicated-server.log");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing, &log);

    let output = run_until_started(&config, &log, false);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty once the log file is configured, got: {}",
        stderr_of(&output)
    );
    let (contents, events) = configured_log(&log);
    assert_eq!(events.len(), 2, "log contents: {contents}");
    let event = &events[0];
    assert_eq!(event["event"], "startup");
    assert_eq!(event["outcome"], "success");
    assert_eq!(event["component"], "dedicated-server");
    assert_eq!(event["level"], "INFO");
    assert_utc_timestamp(event);
    assert_shutdown_event(&events[1]);
    let summary = event["message"]
        .as_str()
        .expect("the default filter must still emit the startup summary");
    assert!(
        summary.contains("listening on 10.0.0.1:7000"),
        "message: {summary}"
    );
    assert!(
        summary.contains(&format!("label={LABEL}")),
        "message: {summary}"
    );
    assert_no_forbidden_data(&contents, &[&config, &log, &encryption, &signing]);
}
