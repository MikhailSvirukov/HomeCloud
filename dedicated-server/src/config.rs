//! Loading and validation of the dedicated server configuration.
//!
//! The configuration describes how the control plane listens, its own key
//! identity, heartbeat timing, and the fixed storage cluster it bootstraps.
//! Validation rejects structurally wrong configurations early so the runtime
//! can assume a usable [`ServerConfig`].

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

/// Environment variable holding the path to the server configuration file.
pub const CONFIG_PATH_ENV: &str = "PICBACKUP_DEDICATED_SERVER_CONFIG";

/// Configuration file name used when [`CONFIG_PATH_ENV`] is not set.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "dedicated-server.toml";

const FIELD_IDENTITY_LABEL: &str = "identity.label";
const FIELD_HEARTBEAT_INTERVAL_MS: &str = "heartbeat.interval_ms";
const FIELD_HEARTBEAT_TIMEOUT_MS: &str = "heartbeat.timeout_ms";
const FIELD_HEARTBEAT_MISS_THRESHOLD: &str = "heartbeat.miss_threshold";
const FIELD_CLUSTER_NODES: &str = "cluster.nodes";
const FIELD_NODE_LABEL: &str = "cluster.nodes.label";
const FIELD_NODE_ADDRESS: &str = "cluster.nodes.address";
const FIELD_NODE_ENCRYPTION_PUBLIC_KEY: &str = "cluster.nodes.encryption_public_key";
const FIELD_NODE_SIGNING_PUBLIC_KEY: &str = "cluster.nodes.signing_public_key";

/// Top-level configuration of the dedicated server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Address the control plane listens on.
    pub listen: SocketAddr,
    /// Key identity of this dedicated server.
    pub identity: IdentityConfig,
    /// Heartbeat timing towards the storage cluster.
    pub heartbeat: HeartbeatConfig,
    /// Fixed storage cluster bootstrap information.
    pub cluster: ClusterConfig,
}

/// Key identity of the dedicated server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityConfig {
    /// Human-readable label identifying this node.
    pub label: String,
    /// Path to the private key used to decrypt incoming messages.
    pub encryption_private_key: PathBuf,
    /// Path to the private key used to sign outgoing messages.
    pub signing_private_key: PathBuf,
}

/// Heartbeat timing towards the storage cluster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeartbeatConfig {
    /// Send interval in milliseconds.
    pub interval_ms: u64,
    /// Time to wait for a response before a heartbeat counts as missed, in milliseconds.
    pub timeout_ms: u64,
    /// Number of consecutive missed heartbeats tolerated before the node is considered down.
    pub miss_threshold: u32,
}

/// Fixed storage cluster bootstrap information.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterConfig {
    /// Storage nodes that make up the cluster.
    pub nodes: Vec<ClusterNode>,
}

/// A single storage node known at bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterNode {
    /// Human-readable label identifying the node.
    pub label: String,
    /// Address of the node.
    pub address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub encryption_public_key: String,
    /// Public key used to verify messages signed by the node.
    pub signing_public_key: String,
}

/// Errors produced while loading or validating a [`ServerConfig`].
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Reading the configuration file failed.
    #[error("failed to read configuration file '{path}': {source}")]
    Read {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The TOML input could not be parsed into the configuration schema.
    #[error("failed to parse configuration: {source}")]
    Parse {
        /// Underlying TOML parse failure.
        #[source]
        source: toml::de::Error,
    },
    /// The parsed configuration is structurally wrong for the v1 control plane.
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl HeartbeatConfig {
    /// Heartbeat send interval.
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }

    /// Time to wait for a heartbeat response before it counts as missed.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

impl ServerConfig {
    /// Parse and validate a configuration from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(input).map_err(|source| ConfigError::Parse { source })?;
        config.validate()?;
        Ok(config)
    }

    /// Read, parse and validate a configuration file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let input = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&input)
    }

    /// Apply every structural validation rule to the parsed configuration.
    fn validate(&self) -> Result<(), ConfigError> {
        validate_identity(&self.identity)?;
        validate_heartbeat(&self.heartbeat)?;
        validate_cluster(&self.cluster)
    }
}

fn validate_identity(identity: &IdentityConfig) -> Result<(), ConfigError> {
    if identity.label.is_empty() {
        Err(ConfigError::Invalid(format!(
            "{FIELD_IDENTITY_LABEL} must not be empty"
        )))
    } else {
        Ok(())
    }
}

fn validate_heartbeat(heartbeat: &HeartbeatConfig) -> Result<(), ConfigError> {
    if heartbeat.interval_ms == 0 {
        Err(ConfigError::Invalid(format!(
            "{FIELD_HEARTBEAT_INTERVAL_MS} must be greater than zero"
        )))
    } else if heartbeat.timeout_ms == 0 {
        Err(ConfigError::Invalid(format!(
            "{FIELD_HEARTBEAT_TIMEOUT_MS} must be greater than zero"
        )))
    } else if heartbeat.miss_threshold == 0 {
        Err(ConfigError::Invalid(format!(
            "{FIELD_HEARTBEAT_MISS_THRESHOLD} must be greater than zero"
        )))
    } else {
        Ok(())
    }
}

fn validate_cluster(cluster: &ClusterConfig) -> Result<(), ConfigError> {
    if cluster.nodes.is_empty() {
        return Err(ConfigError::Invalid(format!(
            "{FIELD_CLUSTER_NODES} must not be empty"
        )));
    }
    validate_node_fields(&cluster.nodes)?;
    validate_node_labels_unique(&cluster.nodes)?;
    validate_node_addresses_unique(&cluster.nodes)
}

fn validate_node_fields(nodes: &[ClusterNode]) -> Result<(), ConfigError> {
    for node in nodes {
        if node.label.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "{FIELD_NODE_LABEL} must not be empty"
            )));
        } else if node.encryption_public_key.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "{FIELD_NODE_ENCRYPTION_PUBLIC_KEY} must not be empty"
            )));
        } else if node.signing_public_key.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "{FIELD_NODE_SIGNING_PUBLIC_KEY} must not be empty"
            )));
        }
    }
    Ok(())
}

fn validate_node_labels_unique(nodes: &[ClusterNode]) -> Result<(), ConfigError> {
    if all_unique(nodes.iter().map(|node| node.label.as_str())) {
        Ok(())
    } else {
        Err(ConfigError::Invalid(format!(
            "{FIELD_NODE_LABEL} values must be unique"
        )))
    }
}

fn validate_node_addresses_unique(nodes: &[ClusterNode]) -> Result<(), ConfigError> {
    if all_unique(nodes.iter().map(|node| node.address)) {
        Ok(())
    } else {
        Err(ConfigError::Invalid(format!(
            "{FIELD_NODE_ADDRESS} values must be unique"
        )))
    }
}

fn all_unique<T>(values: impl IntoIterator<Item = T>) -> bool
where
    T: Eq + std::hash::Hash,
{
    let mut seen = HashSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = include_str!("../tests/fixtures/valid_server.toml");

    fn valid_config() -> ServerConfig {
        ServerConfig::from_toml_str(VALID_TOML).expect("valid fixture must parse")
    }

    fn first_node_mut(config: &mut ServerConfig) -> &mut ClusterNode {
        config
            .cluster
            .nodes
            .first_mut()
            .expect("valid fixture must contain a first node")
    }

    #[test]
    fn parses_valid_configuration() {
        let config = valid_config();

        assert_eq!(config.listen, "10.0.0.1:7000".parse().expect("valid addr"));
        assert_eq!(config.identity.label, "dedicated-1");
        assert_eq!(config.heartbeat.interval_ms, 1000);
        assert_eq!(config.heartbeat.timeout_ms, 500);
        assert_eq!(config.heartbeat.miss_threshold, 3);
        assert_eq!(config.cluster.nodes.len(), 2);
        assert_eq!(config.cluster.nodes[1].label, "storage-2");
    }

    #[test]
    fn rejects_empty_identity_label() {
        let mut config = valid_config();
        config.identity.label = String::new();

        let error = config.validate().expect_err("empty label must fail");

        assert!(
            matches!(error, ConfigError::Invalid(message) if message.contains(FIELD_IDENTITY_LABEL))
        );
    }

    #[test]
    fn rejects_zero_heartbeat_interval() {
        let mut config = valid_config();
        config.heartbeat.interval_ms = 0;

        let error = config.validate().expect_err("zero interval must fail");

        assert!(matches!(
            error,
            ConfigError::Invalid(message) if message.contains(FIELD_HEARTBEAT_INTERVAL_MS)
        ));
    }

    #[test]
    fn rejects_zero_heartbeat_timeout() {
        let mut config = valid_config();
        config.heartbeat.timeout_ms = 0;

        let error = config.validate().expect_err("zero timeout must fail");

        assert!(matches!(
            error,
            ConfigError::Invalid(message) if message.contains(FIELD_HEARTBEAT_TIMEOUT_MS)
        ));
    }

    #[test]
    fn rejects_zero_miss_threshold() {
        let mut config = valid_config();
        config.heartbeat.miss_threshold = 0;

        let error = config
            .validate()
            .expect_err("zero miss threshold must fail");

        assert!(matches!(
            error,
            ConfigError::Invalid(message) if message.contains(FIELD_HEARTBEAT_MISS_THRESHOLD)
        ));
    }

    #[test]
    fn rejects_empty_cluster() {
        let mut config = valid_config();
        config.cluster.nodes.clear();

        let error = config.validate().expect_err("empty cluster must fail");

        assert!(
            matches!(error, ConfigError::Invalid(message) if message.contains(FIELD_CLUSTER_NODES))
        );
    }

    #[test]
    fn rejects_empty_node_label() {
        let mut config = valid_config();
        first_node_mut(&mut config).label = String::new();

        let error = config.validate().expect_err("empty node label must fail");

        assert!(
            matches!(error, ConfigError::Invalid(message) if message.contains(FIELD_NODE_LABEL))
        );
    }

    #[test]
    fn rejects_duplicate_node_labels() {
        let mut config = valid_config();
        let duplicate_label = config.cluster.nodes[0].label.clone();
        config.cluster.nodes[1].label = duplicate_label;

        let error = config.validate().expect_err("duplicate labels must fail");

        assert!(
            matches!(error, ConfigError::Invalid(message) if message.contains(FIELD_NODE_LABEL))
        );
    }

    #[test]
    fn rejects_duplicate_node_addresses() {
        let mut config = valid_config();
        let duplicate_address = config.cluster.nodes[0].address;
        config.cluster.nodes[1].address = duplicate_address;

        let error = config
            .validate()
            .expect_err("duplicate addresses must fail");

        assert!(
            matches!(error, ConfigError::Invalid(message) if message.contains(FIELD_NODE_ADDRESS))
        );
    }

    #[test]
    fn rejects_empty_node_encryption_public_key() {
        let mut config = valid_config();
        first_node_mut(&mut config).encryption_public_key = String::new();

        let error = config
            .validate()
            .expect_err("empty encryption key must fail");

        assert!(matches!(
            error,
            ConfigError::Invalid(message) if message.contains(FIELD_NODE_ENCRYPTION_PUBLIC_KEY)
        ));
    }

    #[test]
    fn rejects_empty_node_signing_public_key() {
        let mut config = valid_config();
        first_node_mut(&mut config).signing_public_key = String::new();

        let error = config.validate().expect_err("empty signing key must fail");

        assert!(matches!(
            error,
            ConfigError::Invalid(message) if message.contains(FIELD_NODE_SIGNING_PUBLIC_KEY)
        ));
    }

    #[test]
    fn heartbeat_accessors_convert_milliseconds_to_duration() {
        let heartbeat = HeartbeatConfig {
            interval_ms: 1500,
            timeout_ms: 250,
            miss_threshold: 3,
        };

        assert_eq!(heartbeat.interval(), Duration::from_millis(1500));
        assert_eq!(heartbeat.timeout(), Duration::from_millis(250));
    }
}
