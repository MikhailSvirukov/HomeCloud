//! Binary-level integration tests for identity-aware startup wiring.
//!
//! These tests run the compiled `dedicated-server` binary end to end and assert
//! on its exit status and output. The identity module's unit tests already cover
//! `ServerIdentity::load` in isolation, so this file drives the process instead.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dedicated_server::CONFIG_PATH_ENV;

/// Binary under test, provided by Cargo for integration tests.
const BINARY: &str = env!("CARGO_BIN_EXE_dedicated-server");
/// Environment variable the binary reads its log filter from.
const RUST_LOG_ENV: &str = "RUST_LOG";
/// Log level that keeps the startup summary and error events visible.
const RUST_LOG_LEVEL: &str = "info";
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

/// Write a complete, valid configuration pointing at the given identity paths.
fn write_config(dir: &TempDir, encryption: &Path, signing: &Path) -> PathBuf {
    let config = CONFIG_TEMPLATE
        .replace(
            ENCRYPTION_KEY_PLACEHOLDER,
            &encryption.display().to_string(),
        )
        .replace(SIGNING_KEY_PLACEHOLDER, &signing.display().to_string())
        .replace(LABEL_PLACEHOLDER, LABEL);
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

/// Run the binary without `RUST_LOG` so its built-in default filter applies.
///
/// Mirrors [`run_with_config`] instead of taking a flag so the two cases stay
/// readable; `env_remove` also drops any ambient value from the test runner.
fn run_with_config_without_log_filter(config: &Path) -> Output {
    Command::new(BINARY)
        .env(CONFIG_PATH_ENV, config)
        .env_remove(RUST_LOG_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("dedicated-server binary must run")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn valid_config_and_distinct_keys_log_one_summary_and_exit_zero() {
    let dir = TempDir::new("valid");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing);

    let output = run_with_config(&config);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    let summary = stderr_of(&output);
    assert!(
        summary.contains("listening on 10.0.0.1:7000"),
        "stderr: {summary}"
    );
    assert!(
        summary.contains(&format!("label={LABEL}")),
        "stderr: {summary}"
    );
    assert!(summary.contains("nodes=2"), "stderr: {summary}");
    assert!(summary.contains("interval_ms=1000"), "stderr: {summary}");
    assert!(summary.contains("timeout_ms=500"), "stderr: {summary}");
    assert!(summary.contains("miss_threshold=3"), "stderr: {summary}");
    assert!(
        summary.contains(&format!("encryption_key_bytes={}", ENCRYPTION_KEY.len())),
        "stderr: {summary}"
    );
    assert!(
        summary.contains(&format!("signing_key_bytes={}", SIGNING_KEY.len())),
        "stderr: {summary}"
    );
}

#[test]
fn missing_encryption_key_file_fails_and_names_key_and_config_paths() {
    let dir = TempDir::new("missing-encryption");
    let missing = dir.file("absent-encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &missing, &signing);

    let output = run_with_config(&config);

    assert_eq!(
        output.status.code(),
        Some(1),
        "the process must exit with the failure code, stderr: {}",
        stderr_of(&output)
    );
    let error = stderr_of(&output);
    assert!(
        error.contains(&missing.display().to_string()),
        "stderr must name the missing key: {error}"
    );
    assert!(
        error.contains(&config.display().to_string()),
        "stderr must name the configuration: {error}"
    );
}

#[test]
fn unset_log_filter_still_logs_startup_summary_and_exits_zero() {
    let dir = TempDir::new("default-log-filter");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing);

    let output = run_with_config_without_log_filter(&config);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty, got: {:?}",
        stdout_of(&output)
    );
    let summary = stderr_of(&output);
    assert!(
        summary.contains("listening on 10.0.0.1:7000"),
        "the default filter must still emit the startup summary: {summary}"
    );
    assert!(
        summary.contains(&format!("label={LABEL}")),
        "stderr: {summary}"
    );
}
