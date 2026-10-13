//! Loading and validation of the dedicated server configuration.
//!
//! The configuration describes how the control plane listens, its own key
//! identity, heartbeat timing, and the fixed storage cluster it bootstraps.
//! `serde` enforces the shape of every field while parsing, and the cross-field
//! rules (non-empty and unique entries) are checked afterwards so the runtime
//! can assume a usable [`ServerConfig`].

use std::collections::BTreeSet;
use std::fmt;
use std::net::SocketAddr;
use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

/// Environment variable holding the path to the server configuration file.
pub const CONFIG_PATH_ENV: &str = "PICBACKUP_DEDICATED_SERVER_CONFIG";

/// Configuration file name used when [`CONFIG_PATH_ENV`] is not set.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "dedicated-server.toml";

/// Result of loading or validating a [`ServerConfig`].
type Result<T> = core::result::Result<T, ConfigError>;

/// Top-level configuration of the dedicated server.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Address the control plane listens on.
    pub(crate) listen: SocketAddr,
    /// Key identity of this dedicated server.
    pub(crate) identity: IdentityConfig,
    /// Heartbeat timing towards the storage cluster.
    pub(crate) heartbeat: HeartbeatConfig,
    /// Fixed storage cluster bootstrap information.
    pub(crate) cluster: ClusterConfig,
}

/// Key identity of the dedicated server.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdentityConfig {
    /// Human-readable label identifying this node.
    pub(crate) label: String,
    /// Path to the private key used to decrypt incoming messages.
    pub(crate) encryption_private_key: PathBuf,
    /// Path to the private key used to sign outgoing messages.
    pub(crate) signing_private_key: PathBuf,
}

/// Heartbeat timing towards the storage cluster.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeartbeatConfig {
    /// Send interval in milliseconds.
    pub(crate) interval_ms: NonZeroU64,
    /// Time to wait for a response before a heartbeat counts as missed, in milliseconds.
    pub(crate) timeout_ms: NonZeroU64,
    /// Number of consecutive missed heartbeats tolerated before the node is considered down.
    pub(crate) miss_threshold: NonZeroU32,
}

/// Fixed storage cluster bootstrap information.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterConfig {
    /// Storage nodes that make up the cluster.
    pub(crate) nodes: Vec<ClusterNode>,
}

/// A single storage node known at bootstrap.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterNode {
    /// Human-readable label identifying the node.
    pub(crate) label: String,
    /// Address of the node.
    pub(crate) address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub(crate) encryption_public_key: String,
    /// Public key used to verify messages signed by the node.
    pub(crate) signing_public_key: String,
}

/// Field of a [`ClusterNode`] that a configuration rule rejected.
#[derive(Debug)]
pub enum NodeField {
    /// The node's human-readable label.
    Label,
    /// The node's address.
    Address,
    /// The node's encryption public key.
    EncryptionPublicKey,
    /// The node's signing public key.
    SigningPublicKey,
}

impl fmt::Display for NodeField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Label => "label",
            Self::Address => "address",
            Self::EncryptionPublicKey => "encryption_public_key",
            Self::SigningPublicKey => "signing_public_key",
        };
        formatter.write_str(name)
    }
}

/// Errors produced while loading or validating a [`ServerConfig`].
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Reading the configuration file failed.
    #[error("failed to read configuration file '{path}': {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The TOML input could not be parsed into the configuration schema.
    #[error("failed to parse configuration: {source}")]
    Parse {
        #[source]
        source: toml::de::Error,
    },
    /// [`IdentityConfig::label`] is empty.
    #[error("identity.label must not be empty")]
    EmptyIdentityLabel,
    /// [`ClusterConfig::nodes`] is empty.
    #[error("cluster.nodes must not be empty")]
    EmptyCluster,
    /// A required [`ClusterNode`] field is empty.
    #[error("cluster.nodes[{index}].{field} must not be empty")]
    EmptyNodeField { index: usize, field: NodeField },
    /// Two [`ClusterNode`] entries share a value for the same field.
    #[error("cluster.nodes {field} values must be unique")]
    DuplicateNodeField { field: NodeField },
}

impl HeartbeatConfig {
    /// Heartbeat send interval.
    pub(crate) fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms.get())
    }

    /// Time to wait for a heartbeat response before it counts as missed.
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.get())
    }
}

impl ServerConfig {
    /// Parse and validate a configuration from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self> {
        let config: Self = toml::from_str(input).map_err(|source| ConfigError::Parse { source })?;
        config.validate()?;
        Ok(config)
    }

    /// Read, parse and validate a configuration file.
    pub fn load(path: &Path) -> Result<Self> {
        std::fs::read_to_string(path)
            .map_err(|source| ConfigError::Read {
                path: path.to_path_buf(),
                source,
            })
            .and_then(|input| Self::from_toml_str(&input))
    }

    /// Label and private key paths needed to load this server's identity.
    pub fn identity_key_material(&self) -> (&str, &Path, &Path) {
        (
            &self.identity.label,
            &self.identity.encryption_private_key,
            &self.identity.signing_private_key,
        )
    }

    /// Apply every structural validation rule to the parsed configuration.
    fn validate(&self) -> Result<()> {
        validate_identity(&self.identity).and_then(|()| validate_cluster(&self.cluster))
    }
}

fn validate_identity(identity: &IdentityConfig) -> Result<()> {
    if identity.label.is_empty() {
        return Err(ConfigError::EmptyIdentityLabel);
    }
    Ok(())
}

fn validate_cluster(cluster: &ClusterConfig) -> Result<()> {
    (!cluster.nodes.is_empty())
        .then_some(())
        .ok_or(ConfigError::EmptyCluster)
        .and_then(|()| validate_node_fields(&cluster.nodes))
        .and_then(|()| validate_unique_node_labels(&cluster.nodes))
        .and_then(|()| validate_unique_node_addresses(&cluster.nodes))
}

fn validate_node_fields(nodes: &[ClusterNode]) -> Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        if node.label.is_empty() {
            return Err(ConfigError::EmptyNodeField {
                index,
                field: NodeField::Label,
            });
        }
        if node.encryption_public_key.is_empty() {
            return Err(ConfigError::EmptyNodeField {
                index,
                field: NodeField::EncryptionPublicKey,
            });
        }
        if node.signing_public_key.is_empty() {
            return Err(ConfigError::EmptyNodeField {
                index,
                field: NodeField::SigningPublicKey,
            });
        }
    }
    Ok(())
}

fn validate_unique_node_labels(nodes: &[ClusterNode]) -> Result<()> {
    let mut seen = BTreeSet::new();
    if nodes.iter().all(|node| seen.insert(node.label.as_str())) {
        Ok(())
    } else {
        Err(ConfigError::DuplicateNodeField {
            field: NodeField::Label,
        })
    }
}

fn validate_unique_node_addresses(nodes: &[ClusterNode]) -> Result<()> {
    let mut seen = BTreeSet::new();
    if nodes.iter().all(|node| seen.insert(node.address)) {
        Ok(())
    } else {
        Err(ConfigError::DuplicateNodeField {
            field: NodeField::Address,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SERVER: &str = include_str!("../tests/fixtures/valid_server.toml");
    const EMPTY_IDENTITY_LABEL: &str =
        include_str!("../tests/fixtures/invalid/empty_identity_label.toml");
    const EMPTY_CLUSTER: &str = include_str!("../tests/fixtures/invalid/empty_cluster.toml");
    const EMPTY_NODE_LABEL: &str = include_str!("../tests/fixtures/invalid/empty_node_label.toml");
    const EMPTY_NODE_ENCRYPTION_KEY: &str =
        include_str!("../tests/fixtures/invalid/empty_node_encryption_key.toml");
    const EMPTY_NODE_SIGNING_KEY: &str =
        include_str!("../tests/fixtures/invalid/empty_node_signing_key.toml");
    const DUPLICATE_NODE_LABEL: &str =
        include_str!("../tests/fixtures/invalid/duplicate_node_label.toml");
    const DUPLICATE_NODE_ADDRESS: &str =
        include_str!("../tests/fixtures/invalid/duplicate_node_address.toml");
    const ZERO_INTERVAL: &str = include_str!("../tests/fixtures/invalid/zero_interval.toml");

    #[test]
    fn parses_valid_fixture_with_expected_meaningful_values() {
        let config = ServerConfig::from_toml_str(VALID_SERVER).expect("valid fixture must parse");

        assert_eq!(
            config.listen,
            "10.0.0.1:7000".parse().expect("valid listen address")
        );
        assert_eq!(config.identity.label, "dedicated-1");
        assert_eq!(config.heartbeat.interval().as_millis(), 1000);
        assert_eq!(config.heartbeat.timeout().as_millis(), 500);
        assert_eq!(config.heartbeat.miss_threshold.get(), 3);
        assert_eq!(config.cluster.nodes.len(), 2);
        assert_eq!(config.cluster.nodes[0].label, "storage-1");
        assert_eq!(config.cluster.nodes[1].label, "storage-2");
    }

    #[test]
    fn rejects_empty_identity_label() {
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_IDENTITY_LABEL),
            Err(ConfigError::EmptyIdentityLabel)
        ));
    }

    #[test]
    fn rejects_empty_cluster() {
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_CLUSTER),
            Err(ConfigError::EmptyCluster)
        ));
    }

    #[test]
    fn rejects_empty_node_field() {
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_NODE_LABEL),
            Err(ConfigError::EmptyNodeField {
                field: NodeField::Label,
                ..
            })
        ));
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_NODE_ENCRYPTION_KEY),
            Err(ConfigError::EmptyNodeField {
                field: NodeField::EncryptionPublicKey,
                ..
            })
        ));
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_NODE_SIGNING_KEY),
            Err(ConfigError::EmptyNodeField {
                field: NodeField::SigningPublicKey,
                ..
            })
        ));
    }

    #[test]
    fn rejects_duplicate_node_fields() {
        assert!(matches!(
            ServerConfig::from_toml_str(DUPLICATE_NODE_LABEL),
            Err(ConfigError::DuplicateNodeField {
                field: NodeField::Label
            })
        ));
        assert!(matches!(
            ServerConfig::from_toml_str(DUPLICATE_NODE_ADDRESS),
            Err(ConfigError::DuplicateNodeField {
                field: NodeField::Address
            })
        ));
    }

    #[test]
    fn rejects_zero_heartbeat_interval_as_parse_error() {
        assert!(matches!(
            ServerConfig::from_toml_str(ZERO_INTERVAL),
            Err(ConfigError::Parse { .. })
        ));
    }
}
