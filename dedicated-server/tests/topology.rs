//! Public-API integration tests for the topology served to a new connection.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use dedicated_server::{NodeId, PublicKey, Startup, CONFIG_PATH_ENV};
use serde::Deserialize;

/// Configuration template with placeholders substituted before startup.
const CONFIG_TEMPLATE: &str = include_str!("fixtures/server_config.toml.tmpl");
const ENCRYPTION_KEY_PLACEHOLDER: &str = "{{ENCRYPTION_KEY}}";
const SIGNING_KEY_PLACEHOLDER: &str = "{{SIGNING_KEY}}";
const LABEL_PLACEHOLDER: &str = "{{LABEL}}";
const LOG_PATH_PLACEHOLDER: &str = "{{LOG_PATH}}";
const METRICS_BIND_PLACEHOLDER: &str = "{{METRICS_BIND}}";
const METRICS_BIND: &str = "127.0.0.1:0";
const LABEL: &str = "dedicated-1";
const ENCRYPTION_KEY: &[u8] = b"encryption-key-material-0123456789";
const SIGNING_KEY: &[u8] = b"signing-key-material-abcdefghijklmnopqrstuvwxyz";

/// Manifest entries read back from the configuration template.
#[derive(Deserialize)]
struct TemplateConfig {
    cluster: TemplateCluster,
}

#[derive(Deserialize)]
struct TemplateCluster {
    nodes: Vec<TemplateNode>,
}

#[derive(Deserialize)]
struct TemplateNode {
    sender_id: NodeId,
    label: String,
    address: SocketAddr,
    encryption_public_key: PublicKey,
    signing_public_key: PublicKey,
}

/// Owns a per-test temporary directory and removes it on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dedicated-server-topology-{}-{name}",
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
        .replace(LABEL_PLACEHOLDER, LABEL)
        .replace(
            LOG_PATH_PLACEHOLDER,
            &dir.file("dedicated-server.log").display().to_string(),
        )
        .replace(METRICS_BIND_PLACEHOLDER, METRICS_BIND);
    let path = dir.file("dedicated-server.toml");
    std::fs::write(&path, config).expect("test configuration must be written");
    path
}

fn expected_nodes() -> Vec<TemplateNode> {
    toml::from_str::<TemplateConfig>(CONFIG_TEMPLATE)
        .expect("the configuration template must parse")
        .cluster
        .nodes
}

#[test]
fn topology_serves_every_manifest_node_in_order_with_its_identity_data() {
    let expected = expected_nodes();
    let dir = TempDir::new("topology");
    let encryption = dir.file("encryption.key");
    let signing = dir.file("signing.key");
    std::fs::write(&encryption, ENCRYPTION_KEY).expect("encryption key must be written");
    std::fs::write(&signing, SIGNING_KEY).expect("signing key must be written");
    let config = write_config(&dir, &encryption, &signing);
    std::env::set_var(CONFIG_PATH_ENV, &config);
    let configured = std::env::var_os(CONFIG_PATH_ENV).expect("the configuration path is set");

    let startup = Startup::load(Path::new(&configured)).expect("the test configuration must load");

    let topology = startup.topology();

    assert_eq!(topology.nodes.len(), expected.len());
    for (served, manifest) in topology.nodes.iter().zip(&expected) {
        assert_eq!(served.sender_id.as_bytes(), manifest.sender_id.as_bytes());
        assert_eq!(served.label, manifest.label);
        assert_eq!(served.address, manifest.address);
        assert_eq!(
            served.encryption_public_key.as_bytes(),
            manifest.encryption_public_key.as_bytes()
        );
        assert_eq!(
            served.signing_public_key.as_bytes(),
            manifest.signing_public_key.as_bytes()
        );
    }
}
