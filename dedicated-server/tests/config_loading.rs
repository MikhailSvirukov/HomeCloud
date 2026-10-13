//! Integration tests for configuration loading through the public API.

use std::path::{Path, PathBuf};

use dedicated_server::config::{ConfigError, ServerConfig};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

#[test]
fn load_reads_valid_configuration_and_exposes_expected_values() {
    let config = ServerConfig::load(&fixture("valid_server.toml")).expect("fixture must load");

    assert_eq!(
        config.listen,
        "10.0.0.1:7000".parse().expect("valid listen address")
    );
    assert_eq!(config.identity.label, "dedicated-1");
    assert_eq!(config.heartbeat.interval_ms.get(), 1000);
    assert_eq!(config.heartbeat.timeout_ms.get(), 500);
    assert_eq!(config.heartbeat.miss_threshold.get(), 3);
    assert_eq!(config.cluster.nodes.len(), 2);
    assert_eq!(config.cluster.nodes[0].label, "storage-1");
    assert_eq!(
        config.cluster.nodes[1].address,
        "10.0.0.12:7100".parse().expect("valid node address")
    );
}

#[test]
fn load_missing_file_reports_read_error_with_path() {
    let path = Path::new("/nonexistent/picbackup/dedicated-server.toml");

    assert!(matches!(
        ServerConfig::load(path),
        Err(ConfigError::Read { path: error_path, .. }) if error_path == path
    ));
}

#[test]
fn load_rule_violating_fixture_returns_typed_variant() {
    assert!(matches!(
        ServerConfig::load(&fixture("invalid/empty_identity_label.toml")),
        Err(ConfigError::EmptyIdentityLabel)
    ));
}
