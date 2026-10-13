//! Binary-level integration tests for identity-aware startup wiring.
//!
//! These tests run the compiled `dedicated-server` binary end to end and assert
//! on its exit status and output. The identity module's unit tests already cover
//! `ServerIdentity::load` in isolation, so this file drives the process instead.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Binary under test, provided by Cargo for integration tests.
const BINARY: &str = env!("CARGO_BIN_EXE_dedicated-server");
/// Environment variable the binary reads the configuration path from.
const CONFIG_PATH_ENV: &str = "PICBACKUP_DEDICATED_SERVER_CONFIG";
/// Label configured for the identity under test.
const LABEL: &str = "dedicated-1";
/// Distinct, non-empty key material for the two configured roles.
const ENCRYPTION_KEY: &[u8] = b"encryption-key-material-0123456789";
const SIGNING_KEY: &[u8] = b"signing-key-material-abcdefghijklmnopqrstuvwxyz";

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
    let config = format!(
        r#"listen = "10.0.0.1:7000"

[identity]
label = "{LABEL}"
encryption_private_key = "{}"
signing_private_key = "{}"

[heartbeat]
interval_ms = 1000
timeout_ms = 500
miss_threshold = 3

[[cluster.nodes]]
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "storage-1-encryption-public-key"
signing_public_key = "storage-1-signing-public-key"

[[cluster.nodes]]
label = "storage-2"
address = "10.0.0.12:7100"
encryption_public_key = "storage-2-encryption-public-key"
signing_public_key = "storage-2-signing-public-key"
"#,
        encryption.display(),
        signing.display(),
    );
    let path = dir.file("dedicated-server.toml");
    std::fs::write(&path, config).expect("test configuration must be written");
    path
}

/// Run the binary with the configuration path passed through the environment.
fn run_with_config(config: &Path) -> Output {
    Command::new(BINARY)
        .env(CONFIG_PATH_ENV, config)
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
fn valid_config_and_distinct_keys_print_one_summary_line_and_exit_zero() {
    let dir = TempDir::new("valid");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing);

    let output = run_with_config(&config);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let summary = stdout_of(&output);
    assert_eq!(
        summary.lines().count(),
        1,
        "stdout must be exactly one line: {summary:?}"
    );
    assert!(
        summary.contains(&format!("label={LABEL}")),
        "stdout: {summary}"
    );
    assert!(summary.contains("nodes=2"), "stdout: {summary}");
    assert!(
        summary.contains(&format!("encryption_key_bytes={}", ENCRYPTION_KEY.len())),
        "stdout: {summary}"
    );
    assert!(
        summary.contains(&format!("signing_key_bytes={}", SIGNING_KEY.len())),
        "stdout: {summary}"
    );
    assert!(
        stderr_of(&output).is_empty(),
        "stderr must be empty on success"
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

    assert!(!output.status.success(), "the process must exit non-zero");
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
fn shared_encryption_and_signing_file_fails_and_names_the_shared_path() {
    let dir = TempDir::new("shared");
    let shared = dir.file("shared.key");
    std::fs::write(&shared, ENCRYPTION_KEY).expect("shared key must be written");
    let config = write_config(&dir, &shared, &shared);

    let output = run_with_config(&config);

    assert!(!output.status.success(), "the process must exit non-zero");
    let error = stderr_of(&output);
    assert!(
        error.contains(&shared.display().to_string()),
        "stderr must name the shared key path: {error}"
    );
    assert!(
        error.contains(&config.display().to_string()),
        "stderr must name the configuration: {error}"
    );
}
