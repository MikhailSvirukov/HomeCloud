//! Loading and validation of the dedicated server configuration.
//!
//! The configuration describes how the control plane listens, its own key
//! identity, heartbeat timing, password derivation parameters, the structured
//! log file, and the fixed storage cluster it bootstraps.
//! `serde` enforces the shape of every field while parsing, and the cross-field
//! rules (non-empty and unique entries) are checked afterwards so the runtime
//! can assume a usable [`ServerConfig`].

use std::collections::BTreeSet;
use std::fmt;
use std::net::SocketAddr;
use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use crate::password::Argon2Params;

/// Environment variable holding the path to the server configuration file.
pub const CONFIG_PATH_ENV: &str = "HOMECLOUD_DEDICATED_SERVER_CONFIG";

/// Configuration file name used when [`CONFIG_PATH_ENV`] is not set.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "dedicated-server.toml";

/// Number of bytes in a storage node identifier.
const NODE_ID_BYTES: usize = 16;

/// Number of bytes in a public key.
const PUBLIC_KEY_BYTES: usize = 32;

/// Number of hexadecimal characters used to encode one byte.
const HEX_CHARS_PER_BYTE: usize = 2;

/// Result of loading or validating a [`ServerConfig`].
type Result<T> = core::result::Result<T, ConfigError>;

/// Top-level configuration of the dedicated server.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Address the control plane listens on.
    pub(crate) listen: SocketAddr,
    /// Maximum length of any node or identity label in UTF-8 bytes.
    pub(crate) max_label_bytes: NonZeroUsize,
    /// Largest frame payload the server accepts from a peer, in bytes.
    pub(crate) max_frame_bytes: NonZeroU32,
    /// Key identity of this dedicated server.
    pub(crate) identity: IdentityConfig,
    /// Heartbeat timing towards the storage cluster.
    pub(crate) heartbeat: HeartbeatConfig,
    /// Argon2id parameters and salt length used to derive password verifiers.
    pub(crate) password: PasswordConfig,
    /// Structured log file configuration.
    pub(crate) logging: LoggingConfig,
    /// Prometheus-compatible metrics endpoint.
    pub(crate) metrics: MetricsConfig,
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

/// Argon2id parameters and salt length used to derive password verifiers.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordConfig {
    /// Memory cost in kibibytes.
    pub(crate) memory_kib: NonZeroU32,
    /// Number of passes.
    pub(crate) iterations: NonZeroU32,
    /// Degree of parallelism.
    pub(crate) parallelism: NonZeroU32,
    /// Salt length in bytes.
    pub(crate) salt_length_bytes: NonZeroUsize,
}

/// Prometheus-compatible metrics endpoint configuration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// Address and TCP port the metrics endpoint binds to.
    pub(crate) bind: SocketAddr,
}

/// Structured log file configuration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Path of the structured JSON Lines log file.
    pub(crate) path: PathBuf,
    /// How long a rotated log file is kept, in milliseconds.
    pub(crate) retention_ms: NonZeroU64,
    /// Maximum age of the active log file before it is rotated, in milliseconds.
    pub(crate) max_file_age_ms: NonZeroU64,
    /// Maximum size of one log file in bytes.
    pub(crate) max_file_size_bytes: NonZeroU64,
    /// Maximum total size of the active file and every rotated file, in bytes.
    pub(crate) max_total_size_bytes: NonZeroU64,
}

/// Fixed storage cluster bootstrap information.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterConfig {
    /// Storage nodes that make up the cluster.
    pub(crate) nodes: Vec<ClusterNode>,
}

/// A single storage node known at bootstrap.
// The identifier and key fields are consumed by the topology response.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClusterNode {
    /// Identifier of the node in the static manifest.
    pub(crate) sender_id: NodeId,
    /// Human-readable label identifying the node.
    pub(crate) label: String,
    /// Address of the node.
    pub(crate) address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub(crate) encryption_public_key: PublicKey,
    /// Public key used to verify messages signed by the node.
    pub(crate) signing_public_key: PublicKey,
}

/// Identifier of a storage node in the static manifest (16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId([u8; NODE_ID_BYTES]);

/// A 32-byte public key from the static manifest (X25519 or Ed25519).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey([u8; PUBLIC_KEY_BYTES]);

impl NodeId {
    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; NODE_ID_BYTES] {
        &self.0
    }
}

impl PublicKey {
    /// Raw public key bytes.
    pub fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_hex(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_hex(deserializer).map(Self)
    }
}

fn deserialize_hex<'de, D, const N: usize>(
    deserializer: D,
) -> core::result::Result<[u8; N], D::Error>
where
    D: Deserializer<'de>,
{
    let text = <&str>::deserialize(deserializer)?;
    decode_hex(text).ok_or_else(|| {
        <D::Error as serde::de::Error>::custom(format!(
            "expected {} hexadecimal characters",
            N * HEX_CHARS_PER_BYTE
        ))
    })
}

fn decode_hex<const N: usize>(input: &str) -> Option<[u8; N]> {
    let bytes = input.as_bytes();
    if bytes.len() != N * HEX_CHARS_PER_BYTE {
        return None;
    }

    let mut decoded = [0_u8; N];
    let (pairs, _) = bytes.as_chunks::<HEX_CHARS_PER_BYTE>();
    for (byte, [high, low]) in decoded.iter_mut().zip(pairs) {
        *byte = (hex_nibble(*high)? << 4) | hex_nibble(*low)?;
    }
    Some(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Field of a [`ClusterNode`] that a configuration rule rejected.
#[derive(Debug)]
pub enum NodeField {
    /// The node's identifier.
    SenderId,
    /// The node's human-readable label.
    Label,
    /// The node's address.
    Address,
}

impl fmt::Display for NodeField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::SenderId => "sender_id",
            Self::Label => "label",
            Self::Address => "address",
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
    /// A label exceeds [`ServerConfig::max_label_bytes`] in UTF-8 bytes.
    #[error("label must not exceed {max_bytes} UTF-8 bytes")]
    LabelTooLong { max_bytes: usize },
    /// [`ClusterConfig::nodes`] is empty.
    #[error("cluster.nodes must not be empty")]
    EmptyCluster,
    /// A required [`ClusterNode`] field is empty.
    #[error("cluster.nodes[{index}].{field} must not be empty")]
    EmptyNodeField { index: usize, field: NodeField },
    /// A [`ClusterNode::address`] uses port zero.
    #[error("cluster.nodes[{index}].address port must be in 1..=65535")]
    ZeroNodePort { index: usize },
    /// Two [`ClusterNode`] entries share a value for the same field.
    #[error("cluster.nodes {field} values must be unique")]
    DuplicateNodeField { field: NodeField },
    /// The total log size limit cannot hold even one maximum-size log file.
    #[error(
        "logging.max_total_size_bytes ({max_total_size_bytes}) must be at least \
         logging.max_file_size_bytes ({max_file_size_bytes})"
    )]
    LoggingTotalSizeBelowFileSize {
        /// Configured maximum total log set size.
        max_total_size_bytes: u64,
        /// Configured maximum size of one log file.
        max_file_size_bytes: u64,
    },
    /// [`MetricsConfig::bind`] is not a loopback address.
    #[error("metrics.bind ({bind}) must be a loopback address")]
    MetricsBindNotLoopback {
        /// Configured metrics bind address.
        bind: SocketAddr,
    },
}

impl PasswordConfig {
    /// Derivation parameters taken from this configuration.
    pub(crate) fn params(&self) -> Argon2Params {
        Argon2Params {
            memory_kib: self.memory_kib,
            iterations: self.iterations,
            parallelism: self.parallelism,
            salt_length_bytes: self.salt_length_bytes,
        }
    }
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
    /// Largest frame payload the server accepts from a peer, in bytes.
    ///
    /// The receive limit is checked against the announced frame length before
    /// the payload buffer is allocated (`INT-NET-004`).
    pub fn max_frame_bytes(&self) -> NonZeroU32 {
        self.max_frame_bytes
    }

    /// Parse and validate a configuration from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self> {
        let config: Self = toml::from_str(input).map_err(|source| ConfigError::Parse { source })?;
        config.validate()?;
        Ok(config)
    }

    /// Structured log file and retention configuration.
    pub fn logging(&self) -> &LoggingConfig {
        &self.logging
    }

    /// Prometheus-compatible metrics endpoint configuration.
    pub fn metrics(&self) -> &MetricsConfig {
        &self.metrics
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

    /// Apply every structural validation rule to the parsed configuration.
    fn validate(&self) -> Result<()> {
        validate_identity(&self.identity, self.max_label_bytes)
            .and_then(|()| validate_cluster(&self.cluster, self.max_label_bytes))
            .and_then(|()| validate_logging(&self.logging))
            .and_then(|()| validate_metrics(&self.metrics))
    }
}

/// The metrics endpoint must stay on loopback until TLS support is configured.
fn validate_metrics(metrics: &MetricsConfig) -> Result<()> {
    if metrics.bind.ip().is_loopback() {
        Ok(())
    } else {
        Err(ConfigError::MetricsBindNotLoopback { bind: metrics.bind })
    }
}

fn validate_logging(logging: &LoggingConfig) -> Result<()> {
    let max_file_size_bytes = logging.max_file_size_bytes.get();
    let max_total_size_bytes = logging.max_total_size_bytes.get();
    if max_total_size_bytes < max_file_size_bytes {
        Err(ConfigError::LoggingTotalSizeBelowFileSize {
            max_total_size_bytes,
            max_file_size_bytes,
        })
    } else {
        Ok(())
    }
}

fn validate_identity(identity: &IdentityConfig, max_label_bytes: NonZeroUsize) -> Result<()> {
    if identity.label.is_empty() {
        return Err(ConfigError::EmptyIdentityLabel);
    }
    validate_label_length(&identity.label, max_label_bytes)
}

fn validate_label_length(label: &str, max_label_bytes: NonZeroUsize) -> Result<()> {
    if label.len() > max_label_bytes.get() {
        Err(ConfigError::LabelTooLong {
            max_bytes: max_label_bytes.get(),
        })
    } else {
        Ok(())
    }
}

fn validate_cluster(cluster: &ClusterConfig, max_label_bytes: NonZeroUsize) -> Result<()> {
    (!cluster.nodes.is_empty())
        .then_some(())
        .ok_or(ConfigError::EmptyCluster)
        .and_then(|()| validate_node_fields(&cluster.nodes, max_label_bytes))
        .and_then(|()| validate_unique_node_labels(&cluster.nodes))
        .and_then(|()| validate_unique_node_addresses(&cluster.nodes))
        .and_then(|()| validate_unique_node_sender_ids(&cluster.nodes))
}

fn validate_node_fields(nodes: &[ClusterNode], max_label_bytes: NonZeroUsize) -> Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        if node.label.is_empty() {
            return Err(ConfigError::EmptyNodeField {
                index,
                field: NodeField::Label,
            });
        }
        validate_label_length(&node.label, max_label_bytes)?;
        if node.address.port() == 0 {
            return Err(ConfigError::ZeroNodePort { index });
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

fn validate_unique_node_sender_ids(nodes: &[ClusterNode]) -> Result<()> {
    let mut seen = BTreeSet::new();
    if nodes.iter().all(|node| seen.insert(node.sender_id)) {
        Ok(())
    } else {
        Err(ConfigError::DuplicateNodeField {
            field: NodeField::SenderId,
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
    const DUPLICATE_NODE_LABEL: &str =
        include_str!("../tests/fixtures/invalid/duplicate_node_label.toml");
    const DUPLICATE_NODE_ADDRESS: &str =
        include_str!("../tests/fixtures/invalid/duplicate_node_address.toml");
    const INVALID_NODE_KEY_HEX: &str =
        include_str!("../tests/fixtures/invalid/invalid_node_key_hex.toml");
    const INVALID_NODE_KEY_NON_HEX: &str =
        include_str!("../tests/fixtures/invalid/invalid_node_key_non_hex.toml");
    const INVALID_NODE_ID_HEX: &str =
        include_str!("../tests/fixtures/invalid/invalid_node_id_hex.toml");
    const LABEL_TOO_LONG: &str = include_str!("../tests/fixtures/invalid/label_too_long.toml");
    const DUPLICATE_NODE_SENDER_ID: &str =
        include_str!("../tests/fixtures/invalid/duplicate_node_sender_id.toml");
    const ZERO_NODE_PORT: &str = include_str!("../tests/fixtures/invalid/zero_node_port.toml");
    const ZERO_INTERVAL: &str = include_str!("../tests/fixtures/invalid/zero_interval.toml");
    const ZERO_PASSWORD_MEMORY: &str =
        include_str!("../tests/fixtures/invalid/zero_password_memory.toml");
    const MISSING_LOGGING: &str = include_str!("../tests/fixtures/invalid/missing_logging.toml");
    const UNKNOWN_LOGGING_KEY: &str =
        include_str!("../tests/fixtures/invalid/unknown_logging_key.toml");

    fn parse_error(input: &str) -> toml::de::Error {
        match ServerConfig::from_toml_str(input) {
            Err(ConfigError::Parse { source }) => source,
            Err(error) => panic!("expected a parse error, got {error}"),
            Ok(_) => panic!("configuration must be rejected"),
        }
    }

    #[test]
    fn parses_valid_fixture_with_expected_meaningful_values() {
        let config = ServerConfig::from_toml_str(VALID_SERVER).expect("valid fixture must parse");

        assert_eq!(
            config.listen,
            "10.0.0.1:7000".parse().expect("valid listen address")
        );
        assert_eq!(config.max_label_bytes.get(), 64);
        assert_eq!(config.max_frame_bytes.get(), 1_048_576);
        assert_eq!(
            config.metrics().bind,
            "127.0.0.1:9101".parse().expect("valid metrics address")
        );
        assert_eq!(config.identity.label, "dedicated-1");
        assert_eq!(config.heartbeat.interval().as_millis(), 1000);
        assert_eq!(config.heartbeat.timeout().as_millis(), 500);
        assert_eq!(config.heartbeat.miss_threshold.get(), 3);
        assert_eq!(config.password.memory_kib.get(), 8);
        assert_eq!(config.password.iterations.get(), 1);
        assert_eq!(config.password.parallelism.get(), 1);
        assert_eq!(config.password.salt_length_bytes.get(), 16);
        assert_eq!(
            config.logging().path,
            Path::new("/var/log/homecloud/dedicated-server.log")
        );
        assert_eq!(config.logging().retention_ms.get(), 604_800_000);
        assert_eq!(config.logging().max_file_age_ms.get(), 86_400_000);
        assert_eq!(config.logging().max_file_size_bytes.get(), 104_857_600);
        assert_eq!(config.logging().max_total_size_bytes.get(), 1_073_741_824);
        assert_eq!(config.cluster.nodes.len(), 2);
        assert_eq!(
            config.cluster.nodes[0].sender_id.as_bytes(),
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
        );
        assert_eq!(config.cluster.nodes[0].label, "storage-1");
        assert_eq!(
            config.cluster.nodes[0].encryption_public_key.as_bytes(),
            &[0x11; PUBLIC_KEY_BYTES]
        );
        assert_eq!(
            config.cluster.nodes[0].signing_public_key.as_bytes(),
            &[0x22; PUBLIC_KEY_BYTES]
        );
        assert_eq!(
            config.cluster.nodes[1].sender_id.as_bytes(),
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]
        );
        assert_eq!(config.cluster.nodes[1].label, "storage-2");
        assert_eq!(
            config.cluster.nodes[1].encryption_public_key.as_bytes(),
            &[0xAA; PUBLIC_KEY_BYTES]
        );
        assert_eq!(
            config.cluster.nodes[1].signing_public_key.as_bytes(),
            &[0xBB; PUBLIC_KEY_BYTES]
        );
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
    fn rejects_empty_node_label() {
        assert!(matches!(
            ServerConfig::from_toml_str(EMPTY_NODE_LABEL),
            Err(ConfigError::EmptyNodeField {
                field: NodeField::Label,
                ..
            })
        ));
    }

    #[test]
    fn rejects_malformed_node_key_hex_as_parse_error() {
        assert!(parse_error(INVALID_NODE_KEY_HEX)
            .to_string()
            .contains("expected 64 hexadecimal characters"));
    }

    #[test]
    fn rejects_non_hexadecimal_node_key_as_parse_error() {
        assert!(parse_error(INVALID_NODE_KEY_NON_HEX)
            .to_string()
            .contains("expected 64 hexadecimal characters"));
    }

    #[test]
    fn rejects_wrong_length_node_id_as_parse_error() {
        assert!(parse_error(INVALID_NODE_ID_HEX)
            .to_string()
            .contains("expected 32 hexadecimal characters"));
    }

    #[test]
    fn rejects_label_longer_than_maximum() {
        assert!(matches!(
            ServerConfig::from_toml_str(LABEL_TOO_LONG),
            Err(ConfigError::LabelTooLong { max_bytes: 8 })
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
    fn rejects_duplicate_node_sender_id() {
        assert!(matches!(
            ServerConfig::from_toml_str(DUPLICATE_NODE_SENDER_ID),
            Err(ConfigError::DuplicateNodeField {
                field: NodeField::SenderId
            })
        ));
    }

    #[test]
    fn rejects_zero_node_port() {
        assert!(matches!(
            ServerConfig::from_toml_str(ZERO_NODE_PORT),
            Err(ConfigError::ZeroNodePort { index: 0 })
        ));
    }

    #[test]
    fn rejects_configuration_without_logging_section() {
        assert!(parse_error(MISSING_LOGGING)
            .to_string()
            .contains("missing field `logging`"));
    }

    #[test]
    fn rejects_unknown_key_inside_logging_section() {
        assert!(parse_error(UNKNOWN_LOGGING_KEY)
            .to_string()
            .contains("unknown field `unexpected`"));
    }

    #[test]
    fn rejects_logging_section_without_retention_period() {
        let document = VALID_SERVER.replace("retention_ms = 604800000\n", "");

        assert!(parse_error(&document)
            .to_string()
            .contains("missing field `retention_ms`"));
    }

    #[test]
    fn rejects_total_log_size_smaller_than_one_file() {
        let document = VALID_SERVER.replace(
            "max_total_size_bytes = 1073741824",
            "max_total_size_bytes = 104857599",
        );

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::LoggingTotalSizeBelowFileSize {
                max_total_size_bytes: 104_857_599,
                max_file_size_bytes: 104_857_600,
            })
        ));
    }

    #[test]
    fn rejects_a_non_loopback_metrics_bind() {
        let document = VALID_SERVER.replace("bind = \"127.0.0.1:9101\"", "bind = \"0.0.0.0:9101\"");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MetricsBindNotLoopback { .. })
        ));
    }

    #[test]
    fn rejects_a_missing_metrics_section() {
        let document = VALID_SERVER.replace("[metrics]\nbind = \"127.0.0.1:9101\"\n", "");

        assert!(parse_error(&document)
            .to_string()
            .contains("missing field `metrics`"));
    }

    #[test]
    fn rejects_a_zero_frame_limit() {
        let document = VALID_SERVER.replace("max_frame_bytes = 1048576", "max_frame_bytes = 0");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn rejects_a_missing_frame_limit() {
        let document = VALID_SERVER.replace("max_frame_bytes = 1048576\n", "");

        assert!(parse_error(&document)
            .to_string()
            .contains("missing field `max_frame_bytes`"));
    }

    #[test]
    fn the_configured_frame_limit_is_the_receive_limit() {
        let document = VALID_SERVER.replace("max_frame_bytes = 1048576", "max_frame_bytes = 4");
        let config = ServerConfig::from_toml_str(&document).expect("the document must parse");

        let mut encoded = Vec::new();
        crate::frame::write_frame(&mut encoded, b"abcd").expect("the frame must be written");
        let mut reader = std::io::Cursor::new(encoded);
        assert_eq!(
            crate::frame::read_frame(&mut reader, config.max_frame_bytes())
                .expect("a frame at the configured limit must be accepted"),
            b"abcd"
        );

        let mut encoded = Vec::new();
        crate::frame::write_frame(&mut encoded, b"abcde").expect("the frame must be written");
        let mut reader = std::io::Cursor::new(encoded);
        assert!(matches!(
            crate::frame::read_frame(&mut reader, config.max_frame_bytes()),
            Err(crate::frame::FrameError::TooLarge {
                announced: 5,
                max: 4
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

    #[test]
    fn rejects_zero_password_memory_as_parse_error() {
        assert!(matches!(
            ServerConfig::from_toml_str(ZERO_PASSWORD_MEMORY),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn password_params_carry_each_configured_value() {
        let document = VALID_SERVER
            .replace("memory_kib = 8", "memory_kib = 24")
            .replace("iterations = 1", "iterations = 3")
            .replace("parallelism = 1", "parallelism = 2")
            .replace("salt_length_bytes = 16", "salt_length_bytes = 20");
        let config = ServerConfig::from_toml_str(&document).expect("valid fixture must parse");

        let params = config.password.params();

        assert_eq!(params.memory_kib.get(), 24);
        assert_eq!(params.iterations.get(), 3);
        assert_eq!(params.parallelism.get(), 2);
        assert_eq!(params.salt_length_bytes.get(), 20);
    }
}
