//! Integration tests for configuration loading through the public API.

use std::path::{Path, PathBuf};

use dedicated_server::config::{
    ClusterConfig, ClusterNode, ConfigError, HeartbeatConfig, IdentityConfig, ServerConfig,
};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_server.toml")
}

#[test]
fn load_reads_full_valid_configuration() {
    let config = ServerConfig::load(&fixture_path()).expect("fixture must load");

    let expected = ServerConfig {
        listen: "10.0.0.1:7000".parse().expect("valid listen address"),
        identity: IdentityConfig {
            label: "dedicated-1".to_owned(),
            encryption_private_key: PathBuf::from("/etc/picbackup/dedicated/encryption.key"),
            signing_private_key: PathBuf::from("/etc/picbackup/dedicated/signing.key"),
        },
        heartbeat: HeartbeatConfig {
            interval_ms: 1000,
            timeout_ms: 500,
            miss_threshold: 3,
        },
        cluster: ClusterConfig {
            nodes: vec![
                ClusterNode {
                    label: "storage-1".to_owned(),
                    address: "10.0.0.11:7100".parse().expect("valid node address"),
                    encryption_public_key: "storage-1-encryption-public-key".to_owned(),
                    signing_public_key: "storage-1-signing-public-key".to_owned(),
                },
                ClusterNode {
                    label: "storage-2".to_owned(),
                    address: "10.0.0.12:7100".parse().expect("valid node address"),
                    encryption_public_key: "storage-2-encryption-public-key".to_owned(),
                    signing_public_key: "storage-2-signing-public-key".to_owned(),
                },
            ],
        },
    };

    assert_eq!(config, expected);
}

#[test]
fn load_missing_file_reports_read_error_with_path() {
    let path = Path::new("/nonexistent/picbackup/dedicated-server.toml");

    match ServerConfig::load(path) {
        Err(ConfigError::Read {
            path: error_path, ..
        }) => assert_eq!(error_path, path),
        other => panic!("expected read error, got {other:?}"),
    }
}

#[test]
fn from_toml_str_rejects_unknown_top_level_key() {
    let input = r#"
listen = "10.0.0.1:7000"
unexpected = true

[identity]
label = "dedicated-1"
encryption_private_key = "/etc/picbackup/dedicated/encryption.key"
signing_private_key = "/etc/picbackup/dedicated/signing.key"

[heartbeat]
interval_ms = 1000
timeout_ms = 500
miss_threshold = 3

[[cluster.nodes]]
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "storage-1-encryption-public-key"
signing_public_key = "storage-1-signing-public-key"
"#;

    assert!(matches!(
        ServerConfig::from_toml_str(input),
        Err(ConfigError::Parse { .. })
    ));
}

#[test]
fn from_toml_str_rejects_missing_required_section() {
    let input = r#"
listen = "10.0.0.1:7000"

[identity]
label = "dedicated-1"
encryption_private_key = "/etc/picbackup/dedicated/encryption.key"
signing_private_key = "/etc/picbackup/dedicated/signing.key"

[[cluster.nodes]]
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "storage-1-encryption-public-key"
signing_public_key = "storage-1-signing-public-key"
"#;

    assert!(matches!(
        ServerConfig::from_toml_str(input),
        Err(ConfigError::Parse { .. })
    ));
}

#[test]
fn from_toml_str_rejects_empty_cluster_nodes() {
    let input = r#"
listen = "10.0.0.1:7000"

[identity]
label = "dedicated-1"
encryption_private_key = "/etc/picbackup/dedicated/encryption.key"
signing_private_key = "/etc/picbackup/dedicated/signing.key"

[heartbeat]
interval_ms = 1000
timeout_ms = 500
miss_threshold = 3

[cluster]
nodes = []
"#;

    match ServerConfig::from_toml_str(input) {
        Err(ConfigError::Invalid(message)) => {
            assert!(
                message.contains("cluster.nodes"),
                "message must name cluster.nodes, got: {message}"
            );
        }
        other => panic!("expected invalid error, got {other:?}"),
    }
}
