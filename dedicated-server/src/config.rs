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

/// Configuration field name of the metrics server certificate path.
const FIELD_TLS_CERTIFICATE_PATH: &str = "certificate_path";

/// Configuration field name of the metrics private key path.
const FIELD_TLS_PRIVATE_KEY_PATH: &str = "private_key_path";

/// Configuration field name of the metrics trust material path.
const FIELD_TLS_TRUST_PATH: &str = "trust_path";

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
    /// Largest encoded logical header the server accepts, in bytes (`CFG-CL-046`).
    pub(crate) max_header_bytes: NonZeroUsize,
    /// Largest PHC verifier in ASCII bytes (`CFG-CL-047`).
    pub(crate) max_verifier_bytes: NonZeroUsize,
    /// Smallest accepted login length in ASCII bytes (`CFG-CL-029`).
    pub(crate) min_login_bytes: NonZeroUsize,
    /// Largest accepted login length in ASCII bytes (`CFG-CL-029`).
    pub(crate) max_login_bytes: NonZeroUsize,
    /// Smallest accepted password length in UTF-8 bytes (`CFG-CL-029`).
    pub(crate) min_password_bytes: NonZeroUsize,
    /// Largest accepted password length in UTF-8 bytes (`CFG-CL-029`).
    pub(crate) max_password_bytes: NonZeroUsize,
    /// Largest accepted media source name in UTF-8 bytes (`CFG-CL-032`).
    pub(crate) max_source_name_bytes: NonZeroUsize,
    /// Largest original or compressed media size in bytes (`CFG-CL-020`).
    pub(crate) max_file_size_bytes: NonZeroU64,
    /// Connect timeout of a dedicated-server connection attempt (`CFG-CL-027`).
    pub(crate) connect_timeout_ms: NonZeroU64,
    /// Timeout of one control inter-server request (`CFG-CL-054`).
    pub(crate) inter_server_timeout_ms: NonZeroU64,
    /// Registration retry budgets (`CFG-CL-042`-`CFG-CL-045`).
    pub(crate) registration: RegistrationConfig,
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

/// Registration retry budgets (`CFG-CL-042`-`CFG-CL-045`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistrationConfig {
    /// Total time allowed for the storage commit stage.
    pub(crate) total_deadline_ms: NonZeroU64,
    /// Time allowed for one storage attempt, connection included.
    pub(crate) attempt_deadline_ms: NonZeroU64,
    /// Maximum number of storage attempts, the first one included.
    pub(crate) attempts: NonZeroU32,
    /// Pause between two storage attempts.
    pub(crate) retry_interval_ms: NonZeroU64,
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
    pub bind: SocketAddr,
    /// TLS material served and advertised by the endpoint.
    ///
    /// Required when [`MetricsConfig::bind`] is not a loopback address; the
    /// endpoint never runs unencrypted outside the host (`NFR-OBS-007`).
    pub tls: Option<MetricsTlsConfig>,
}

/// TLS material for the metrics endpoint (`CFG-OBS-004`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsTlsConfig {
    /// Server certificate chain served by the endpoint.
    pub certificate_path: PathBuf,
    /// Private key of the server certificate.
    pub private_key_path: PathBuf,
    /// Trust material the collecting client uses to verify the server.
    ///
    /// The server validates that the file exists and contains a PEM
    /// certificate so the operator cannot publish an endpoint whose collector
    /// has nothing to verify against.
    pub trust_path: PathBuf,
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
    /// Build an identifier from its raw bytes.
    pub fn from_bytes(bytes: [u8; NODE_ID_BYTES]) -> Self {
        Self(bytes)
    }

    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; NODE_ID_BYTES] {
        &self.0
    }
}

impl PublicKey {
    /// Build a public key from its raw bytes.
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

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
    /// [`MetricsConfig::bind`] uses port zero.
    #[error("metrics.bind ({bind}) must use a port in 1..=65535")]
    MetricsZeroPort {
        /// Configured metrics bind address.
        bind: SocketAddr,
    },
    /// [`MetricsConfig::bind`] is not a loopback address and no TLS is configured.
    #[error("metrics.bind ({bind}) is not loopback; metrics.tls is required")]
    MetricsTlsRequired {
        /// Configured metrics bind address.
        bind: SocketAddr,
    },
    /// A metrics TLS path is empty.
    #[error("metrics.tls.{field} must not be empty")]
    MetricsEmptyTlsPath {
        /// Offending TLS path field.
        field: &'static str,
    },
    /// A numeric setting is outside the approved range (`CFG-CL-049`).
    #[error("{field} is {value}, outside the allowed {min}..={max}")]
    OutOfRange {
        /// Configuration field that violates its range.
        field: &'static str,
        /// Observed value.
        value: u64,
        /// Inclusive minimum.
        min: u64,
        /// Inclusive maximum.
        max: u64,
    },
    /// The heartbeat response timeout is not below the send interval.
    #[error(
        "heartbeat.timeout_ms ({timeout_ms}) must be less than heartbeat.interval_ms ({interval_ms})"
    )]
    HeartbeatTimeoutNotBelowInterval {
        /// Configured response timeout.
        timeout_ms: u64,
        /// Configured send interval.
        interval_ms: u64,
    },
    /// A minimum length is above its maximum.
    #[error("{field} minimum ({min}) must not exceed its maximum ({max})")]
    MinimumAboveMaximum {
        /// Credential whose limits conflict.
        field: &'static str,
        /// Configured minimum.
        min: u64,
        /// Configured maximum.
        max: u64,
    },
    /// The registration attempt deadline exceeds the total deadline.
    #[error(
        "registration.attempt_deadline_ms ({attempt_deadline_ms}) must not exceed registration.total_deadline_ms ({total_deadline_ms})"
    )]
    RegistrationDeadlineOrder {
        /// Configured per-attempt deadline.
        attempt_deadline_ms: u64,
        /// Configured total deadline.
        total_deadline_ms: u64,
    },
    /// The log rotation age exceeds the retention period.
    #[error(
        "logging.max_file_age_ms ({max_file_age_ms}) must not exceed logging.retention_ms ({retention_ms})"
    )]
    RotationExceedsRetention {
        /// Configured rotation age.
        max_file_age_ms: u64,
        /// Configured retention period.
        retention_ms: u64,
    },
    /// The metrics endpoint shares the application port on the same address.
    #[error(
        "metrics.bind ({bind}) must use a different port from listen ({listen}) on the same address"
    )]
    MetricsPortConflict {
        /// Configured metrics bind address.
        bind: SocketAddr,
        /// Configured application listen address.
        listen: SocketAddr,
    },
    /// The maximum logical header does not fit the configured frame body.
    #[error(
        "max_header_bytes ({max_header_bytes}) plus its outer envelope ({overhead}) does not fit max_frame_bytes ({max_frame_bytes})"
    )]
    HeaderDoesNotFitFrame {
        /// Configured logical header limit.
        max_header_bytes: u64,
        /// Worst-case external envelope around the logical header.
        overhead: u64,
        /// Configured frame body limit.
        max_frame_bytes: u64,
    },
    /// An operation's worst-case logical header exceeds the configured limit.
    #[error(
        "the worst-case {operation} logical header ({needed} bytes) does not fit max_header_bytes ({max_header_bytes})"
    )]
    OperationDoesNotFitHeader {
        /// Operation or answer whose header was measured.
        operation: &'static str,
        /// Worst-case encoded size.
        needed: u64,
        /// Configured logical header limit.
        max_header_bytes: u64,
    },
    /// The generated PHC verifier exceeds the configured verifier limit.
    #[error(
        "the generated PHC verifier ({needed} bytes) does not fit max_verifier_bytes ({max_verifier_bytes})"
    )]
    VerifierDoesNotFit {
        /// Worst-case PHC length for the configured parameters.
        needed: u64,
        /// Configured verifier limit.
        max_verifier_bytes: u64,
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
            .and_then(|()| validate_profile(self))
            .and_then(|()| validate_relations(self))
    }
}

/// Approved numerical ranges of the configuration profile (`CFG-CL-049`, `CFG-CL-053`).
fn validate_profile(config: &ServerConfig) -> Result<()> {
    let checks: [(&'static str, u64, u64, u64); 26] = [
        (
            "max_frame_bytes",
            config.max_frame_bytes.get() as u64,
            4_096,
            67_108_864,
        ),
        (
            "max_header_bytes",
            config.max_header_bytes.get() as u64,
            1_024,
            16_777_216,
        ),
        (
            "max_verifier_bytes",
            config.max_verifier_bytes.get() as u64,
            128,
            4_096,
        ),
        (
            "min_login_bytes",
            config.min_login_bytes.get() as u64,
            1,
            255,
        ),
        (
            "max_login_bytes",
            config.max_login_bytes.get() as u64,
            1,
            255,
        ),
        (
            "min_password_bytes",
            config.min_password_bytes.get() as u64,
            1,
            1_024,
        ),
        (
            "max_password_bytes",
            config.max_password_bytes.get() as u64,
            1,
            4_096,
        ),
        (
            "max_source_name_bytes",
            config.max_source_name_bytes.get() as u64,
            1,
            65_535,
        ),
        (
            "max_file_size_bytes",
            config.max_file_size_bytes.get(),
            1,
            i64::MAX as u64,
        ),
        (
            "connect_timeout_ms",
            config.connect_timeout_ms.get(),
            100,
            300_000,
        ),
        (
            "inter_server_timeout_ms",
            config.inter_server_timeout_ms.get(),
            100,
            300_000,
        ),
        (
            "max_label_bytes",
            config.max_label_bytes.get() as u64,
            1,
            4_096,
        ),
        (
            "heartbeat.interval_ms",
            config.heartbeat.interval_ms.get(),
            100,
            3_600_000,
        ),
        (
            "heartbeat.timeout_ms",
            config.heartbeat.timeout_ms.get(),
            100,
            3_600_000,
        ),
        (
            "heartbeat.miss_threshold",
            config.heartbeat.miss_threshold.get() as u64,
            1,
            1_000,
        ),
        (
            "password.parallelism",
            config.password.parallelism.get() as u64,
            1,
            16,
        ),
        (
            "password.iterations",
            config.password.iterations.get() as u64,
            1,
            100,
        ),
        (
            "password.salt_length_bytes",
            config.password.salt_length_bytes.get() as u64,
            16,
            64,
        ),
        (
            "logging.retention_ms",
            config.logging.retention_ms.get(),
            1_000,
            31_536_000_000,
        ),
        (
            "logging.max_file_age_ms",
            config.logging.max_file_age_ms.get(),
            1_000,
            31_536_000_000,
        ),
        (
            "logging.max_file_size_bytes",
            config.logging.max_file_size_bytes.get(),
            4_096,
            1_073_741_824,
        ),
        (
            "logging.max_total_size_bytes",
            config.logging.max_total_size_bytes.get(),
            4_096,
            i64::MAX as u64,
        ),
        (
            "registration.total_deadline_ms",
            config.registration.total_deadline_ms.get(),
            1_000,
            300_000,
        ),
        (
            "registration.attempt_deadline_ms",
            config.registration.attempt_deadline_ms.get(),
            100,
            300_000,
        ),
        (
            "registration.attempts",
            config.registration.attempts.get() as u64,
            1,
            1_000,
        ),
        (
            "registration.retry_interval_ms",
            config.registration.retry_interval_ms.get(),
            1,
            60_000,
        ),
    ];
    for (field, value, min, max) in checks {
        if value < min || value > max {
            return Err(ConfigError::OutOfRange {
                field,
                value,
                min,
                max,
            });
        }
    }

    let memory = config.password.memory_kib.get() as u64;
    let minimum_memory = (config.password.parallelism.get() as u64).saturating_mul(8);
    if memory < minimum_memory || memory > 1_048_576 {
        return Err(ConfigError::OutOfRange {
            field: "password.memory_kib",
            value: memory,
            min: minimum_memory,
            max: 1_048_576,
        });
    }

    Ok(())
}

/// Worst-case size of the registration logical header (`CFG-CL-046`).
fn maximum_registration_header_bytes(config: &ServerConfig) -> u64 {
    // [operation, login, password, [x25519_key, ed25519_key]]
    1 + 5
        + (2 + config.max_login_bytes.get() as u64)
        + (2 + config.max_password_bytes.get() as u64)
        + 1
        + 34
        + 34
}

/// Worst-case size of a full topology answer for the configured cluster (`CFG-CL-046`).
fn maximum_topology_answer_bytes(config: &ServerConfig) -> u64 {
    // [result_code, [nodes]]; one node is
    // [sender_id, label, host, port, x25519_public_key, ed25519_public_key].
    let node = 1 + 18 + 3 + config.max_label_bytes.get() as u64 + 2 + 45 + 5 + 34 + 34;
    1 + 5 + 1 + (config.cluster.nodes.len() as u64).saturating_mul(1 + node)
}

/// Worst-case PHC verifier length the configured Argon2id parameters produce (`CFG-CL-047`).
fn maximum_phc_bytes(config: &ServerConfig) -> u64 {
    let password = &config.password;
    let digits = |value: u64| value.to_string().len() as u64;
    let salt = password.salt_length_bytes.get() as u64;
    // "$argon2id$v=19$m=" (18), ",t=" (3), ",p=" (3), "$" (1), salt, "$" (1), 32-byte hash.
    18 + digits(password.memory_kib.get() as u64)
        + 3
        + digits(password.iterations.get() as u64)
        + 3
        + digits(password.parallelism.get() as u64)
        + 1
        + salt.div_ceil(3) * 4
        + 1
        + 32u64.div_ceil(3) * 4
}
/// Cross-field constraints of the configuration profile (`CFG-CL-051`).
fn validate_relations(config: &ServerConfig) -> Result<()> {
    if config.heartbeat.timeout_ms.get() >= config.heartbeat.interval_ms.get() {
        return Err(ConfigError::HeartbeatTimeoutNotBelowInterval {
            timeout_ms: config.heartbeat.timeout_ms.get(),
            interval_ms: config.heartbeat.interval_ms.get(),
        });
    }
    for (field, min, max) in [
        (
            "login",
            config.min_login_bytes.get(),
            config.max_login_bytes.get(),
        ),
        (
            "password",
            config.min_password_bytes.get(),
            config.max_password_bytes.get(),
        ),
    ] {
        if min > max {
            return Err(ConfigError::MinimumAboveMaximum {
                field,
                min: min as u64,
                max: max as u64,
            });
        }
    }
    let registration = &config.registration;
    if registration.attempt_deadline_ms.get() > registration.total_deadline_ms.get() {
        return Err(ConfigError::RegistrationDeadlineOrder {
            attempt_deadline_ms: registration.attempt_deadline_ms.get(),
            total_deadline_ms: registration.total_deadline_ms.get(),
        });
    }
    if config.logging.max_file_age_ms.get() > config.logging.retention_ms.get() {
        return Err(ConfigError::RotationExceedsRetention {
            max_file_age_ms: config.logging.max_file_age_ms.get(),
            retention_ms: config.logging.retention_ms.get(),
        });
    }
    if config.metrics.bind.port() == config.listen.port()
        && (config.metrics.bind.ip() == config.listen.ip()
            || config.metrics.bind.ip().is_unspecified()
            || config.listen.ip().is_unspecified())
    {
        return Err(ConfigError::MetricsPortConflict {
            bind: config.metrics.bind,
            listen: config.listen,
        });
    }

    // Worst-case outer envelope: seven-element external header with every
    // optional element present, a 64-byte Ed25519 signature, the 32-byte HPKE
    // encapsulated key and the 16-byte AEAD tag.
    let maximum_external_header =
        1 + 5 + 18 + 18 + 5 + 18 + 3 + config.max_label_bytes.get() as u64 + 34;
    let overhead = maximum_external_header + 64 + 32 + 16;
    if (config.max_header_bytes.get() as u64).saturating_add(overhead)
        > config.max_frame_bytes.get() as u64
    {
        return Err(ConfigError::HeaderDoesNotFitFrame {
            max_header_bytes: config.max_header_bytes.get() as u64,
            overhead,
            max_frame_bytes: config.max_frame_bytes.get() as u64,
        });
    }

    let header_limit = config.max_header_bytes.get() as u64;
    let registration = maximum_registration_header_bytes(config);
    if registration > header_limit {
        return Err(ConfigError::OperationDoesNotFitHeader {
            operation: "registration",
            needed: registration,
            max_header_bytes: header_limit,
        });
    }
    let topology = maximum_topology_answer_bytes(config);
    if topology > header_limit {
        return Err(ConfigError::OperationDoesNotFitHeader {
            operation: "topology answer",
            needed: topology,
            max_header_bytes: header_limit,
        });
    }
    let verifier = maximum_phc_bytes(config);
    let verifier_limit = config.max_verifier_bytes.get() as u64;
    if verifier > verifier_limit {
        return Err(ConfigError::VerifierDoesNotFit {
            needed: verifier,
            max_verifier_bytes: verifier_limit,
        });
    }

    Ok(())
}
/// A non-loopback metrics bind requires TLS material; loopback may stay HTTP.
fn validate_metrics(metrics: &MetricsConfig) -> Result<()> {
    if !metrics.bind.ip().is_loopback() && metrics.tls.is_none() {
        return Err(ConfigError::MetricsTlsRequired { bind: metrics.bind });
    }
    validate_metrics_port(metrics).and_then(|()| validate_metrics_tls(metrics))
}

fn validate_metrics_port(metrics: &MetricsConfig) -> Result<()> {
    if metrics.bind.port() == 0 {
        Err(ConfigError::MetricsZeroPort { bind: metrics.bind })
    } else {
        Ok(())
    }
}

fn validate_metrics_tls(metrics: &MetricsConfig) -> Result<()> {
    let Some(tls) = metrics.tls.as_ref() else {
        return Ok(());
    };
    for (field, path) in [
        (FIELD_TLS_CERTIFICATE_PATH, &tls.certificate_path),
        (FIELD_TLS_PRIVATE_KEY_PATH, &tls.private_key_path),
        (FIELD_TLS_TRUST_PATH, &tls.trust_path),
    ] {
        if path.as_os_str().is_empty() {
            return Err(ConfigError::MetricsEmptyTlsPath { field });
        }
    }
    Ok(())
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
        assert_eq!(config.max_frame_bytes.get(), 4_194_304);
        assert_eq!(config.max_header_bytes.get(), 1_048_576);
        assert_eq!(config.max_verifier_bytes.get(), 512);
        assert_eq!(config.max_login_bytes.get(), 64);
        assert_eq!(config.max_file_size_bytes.get(), 10_485_760);
        assert_eq!(config.registration.total_deadline_ms.get(), 30_000);
        assert_eq!(config.registration.attempts.get(), 5);
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
    fn rejects_a_frame_limit_below_the_profile_minimum() {
        let document = VALID_SERVER.replace("max_frame_bytes = 4194304", "max_frame_bytes = 1024");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "max_frame_bytes",
                value: 1024,
                min: 4_096,
                max: 67_108_864,
            })
        ));
    }

    #[test]
    fn rejects_a_header_limit_that_does_not_fit_the_frame_body() {
        let document =
            VALID_SERVER.replace("max_header_bytes = 1048576", "max_header_bytes = 16777216");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::HeaderDoesNotFitFrame { .. })
        ));
    }

    #[test]
    fn rejects_a_heartbeat_timeout_not_below_the_interval() {
        let document = VALID_SERVER.replace("timeout_ms = 500", "timeout_ms = 1000");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::HeartbeatTimeoutNotBelowInterval {
                timeout_ms: 1_000,
                interval_ms: 1_000,
            })
        ));
    }

    #[test]
    fn rejects_a_login_minimum_above_the_maximum() {
        let document = VALID_SERVER.replace("min_login_bytes = 1", "min_login_bytes = 65");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MinimumAboveMaximum { field: "login", .. })
        ));
    }

    #[test]
    fn rejects_a_registration_attempt_deadline_above_the_total() {
        let document =
            VALID_SERVER.replace("attempt_deadline_ms = 10000", "attempt_deadline_ms = 40000");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::RegistrationDeadlineOrder { .. })
        ));
    }

    #[test]
    fn rejects_a_rotation_age_above_the_retention_period() {
        let document =
            VALID_SERVER.replace("max_file_age_ms = 86400000", "max_file_age_ms = 700000000");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::RotationExceedsRetention { .. })
        ));
    }

    #[test]
    fn rejects_a_metrics_port_shared_with_the_application_port() {
        let document = VALID_SERVER
            .replace("listen = \"10.0.0.1:7000\"", "listen = \"127.0.0.1:7000\"")
            .replace("bind = \"127.0.0.1:9101\"", "bind = \"127.0.0.1:7000\"");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MetricsPortConflict { .. })
        ));
    }

    #[test]
    fn rejects_a_header_limit_below_the_profile_minimum() {
        let document = VALID_SERVER.replace("max_header_bytes = 1048576", "max_header_bytes = 512");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "max_header_bytes",
                value: 512,
                min: 1_024,
                max: 16_777_216,
            })
        ));
    }

    #[test]
    fn rejects_a_verifier_limit_below_the_profile_minimum() {
        let document = VALID_SERVER.replace("max_verifier_bytes = 512", "max_verifier_bytes = 64");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "max_verifier_bytes",
                value: 64,
                min: 128,
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_connect_timeout_below_the_profile_minimum() {
        let document = VALID_SERVER.replace("connect_timeout_ms = 5000", "connect_timeout_ms = 50");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "connect_timeout_ms",
                value: 50,
                min: 100,
                ..
            })
        ));
    }

    #[test]
    fn rejects_an_inter_server_timeout_above_the_profile_maximum() {
        let document = VALID_SERVER.replace(
            "inter_server_timeout_ms = 10000",
            "inter_server_timeout_ms = 300001",
        );
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "inter_server_timeout_ms",
                value: 300_001,
                max: 300_000,
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_registration_attempt_count_above_the_profile_maximum() {
        let document = VALID_SERVER.replace("attempts = 5", "attempts = 1001");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "registration.attempts",
                value: 1_001,
                max: 1_000,
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_salt_that_the_phc_record_cannot_carry() {
        let document = VALID_SERVER.replace("salt_length_bytes = 16", "salt_length_bytes = 49");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "password.salt_length_bytes",
                value: 49,
                max: 48,
                ..
            })
        ));
    }

    #[test]
    fn rejects_memory_below_eight_times_the_parallelism() {
        let document = VALID_SERVER
            .replace("memory_kib = 8", "memory_kib = 64")
            .replace("parallelism = 1", "parallelism = 16");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "password.memory_kib",
                value: 64,
                min: 128,
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_retention_below_the_profile_minimum() {
        let document = VALID_SERVER.replace("retention_ms = 604800000", "retention_ms = 500");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OutOfRange {
                field: "logging.retention_ms",
                value: 500,
                min: 1_000,
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_password_minimum_above_the_maximum() {
        let document = VALID_SERVER.replace("min_password_bytes = 8", "min_password_bytes = 2048");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MinimumAboveMaximum {
                field: "password",
                ..
            })
        ));
    }

    #[test]
    fn the_header_envelope_bound_is_exact() {
        let base = VALID_SERVER.replace("max_frame_bytes = 4194304", "max_frame_bytes = 4096");
        let accepted = base.replace("max_header_bytes = 1048576", "max_header_bytes = 3818");
        assert!(
            ServerConfig::from_toml_str(&accepted).is_ok(),
            "a header that exactly fits the frame must be accepted"
        );

        let rejected = base.replace("max_header_bytes = 1048576", "max_header_bytes = 3819");
        assert!(matches!(
            ServerConfig::from_toml_str(&rejected),
            Err(ConfigError::HeaderDoesNotFitFrame { .. })
        ));
    }

    #[test]
    fn rejects_a_registration_header_above_the_header_limit() {
        let document =
            VALID_SERVER.replace("max_header_bytes = 1048576", "max_header_bytes = 1024");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OperationDoesNotFitHeader {
                operation: "registration",
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_topology_answer_above_the_header_limit() {
        // A 4096-byte label makes one node 4238 bytes, so a two-node answer is
        // far above the 1200-byte header limit, while the registration header
        // (1167 bytes) still fits.
        let document = VALID_SERVER
            .replace("max_header_bytes = 1048576", "max_header_bytes = 1200")
            .replace("max_label_bytes = 64", "max_label_bytes = 4096");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::OperationDoesNotFitHeader {
                operation: "topology answer",
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_phc_that_does_not_fit_the_verifier_limit() {
        // A 48-byte salt produces a 137-byte PHC, above a 128-byte limit.
        let document = VALID_SERVER
            .replace("max_verifier_bytes = 512", "max_verifier_bytes = 128")
            .replace("salt_length_bytes = 16", "salt_length_bytes = 48");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::VerifierDoesNotFit { .. })
        ));
    }

    #[test]
    fn rejects_a_metrics_port_shared_with_a_wildcard_application_address() {
        let document =
            VALID_SERVER.replace("listen = \"10.0.0.1:7000\"", "listen = \"0.0.0.0:9101\"");
        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MetricsPortConflict { .. })
        ));
    }
    #[test]
    fn rejects_a_non_loopback_metrics_bind_without_tls() {
        let document = VALID_SERVER.replace("bind = \"127.0.0.1:9101\"", "bind = \"0.0.0.0:9101\"");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MetricsTlsRequired { .. })
        ));
    }

    #[test]
    fn accepts_a_non_loopback_metrics_bind_with_tls() {
        let document = VALID_SERVER.replace(
            "bind = \"127.0.0.1:9101\"",
            "bind = \"0.0.0.0:9101\"\n\n[metrics.tls]\ncertificate_path = \"/etc/homecloud/metrics/server.crt\"\nprivate_key_path = \"/etc/homecloud/metrics/server.key\"\ntrust_path = \"/etc/homecloud/metrics/ca.crt\"",
        );

        assert!(ServerConfig::from_toml_str(&document).is_ok());
    }

    #[test]
    fn rejects_an_empty_metrics_tls_path() {
        for (empty_field, section) in [
            (
                "certificate_path",
                "[metrics.tls]\ncertificate_path = \"\"\nprivate_key_path = \"/etc/homecloud/metrics/server.key\"\ntrust_path = \"/etc/homecloud/metrics/ca.crt\"",
            ),
            (
                "private_key_path",
                "[metrics.tls]\ncertificate_path = \"/etc/homecloud/metrics/server.crt\"\nprivate_key_path = \"\"\ntrust_path = \"/etc/homecloud/metrics/ca.crt\"",
            ),
            (
                "trust_path",
                "[metrics.tls]\ncertificate_path = \"/etc/homecloud/metrics/server.crt\"\nprivate_key_path = \"/etc/homecloud/metrics/server.key\"\ntrust_path = \"\"",
            ),
        ] {
            let document = VALID_SERVER.replace(
                "bind = \"127.0.0.1:9101\"",
                &format!("bind = \"127.0.0.1:9101\"\n\n{section}"),
            );

            assert!(matches!(
                ServerConfig::from_toml_str(&document),
                Err(ConfigError::MetricsEmptyTlsPath { field }) if field == empty_field
            ));
        }
    }

    #[test]
    fn rejects_a_zero_metrics_port() {
        let document = VALID_SERVER.replace("bind = \"127.0.0.1:9101\"", "bind = \"127.0.0.1:0\"");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::MetricsZeroPort { .. })
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
        let document = VALID_SERVER.replace("max_frame_bytes = 4194304", "max_frame_bytes = 0");

        assert!(matches!(
            ServerConfig::from_toml_str(&document),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn rejects_a_missing_frame_limit() {
        let document = VALID_SERVER.replace("max_frame_bytes = 4194304\n", "");

        assert!(parse_error(&document)
            .to_string()
            .contains("missing field `max_frame_bytes`"));
    }

    #[test]
    fn the_configured_frame_limit_is_the_receive_limit() {
        let document = VALID_SERVER
            .replace("max_frame_bytes = 4194304", "max_frame_bytes = 4096")
            .replace("max_header_bytes = 1048576", "max_header_bytes = 1024");
        let config = ServerConfig::from_toml_str(&document).expect("the document must parse");

        let mut encoded = Vec::new();
        crate::frame::write_frame(&mut encoded, &[0x11; 4096]).expect("the frame must be written");
        let mut reader = std::io::Cursor::new(encoded);
        assert_eq!(
            crate::frame::read_frame(&mut reader, config.max_frame_bytes())
                .expect("a frame at the configured limit must be accepted")
                .len(),
            4096
        );

        let mut encoded = Vec::new();
        crate::frame::write_frame(&mut encoded, &[0x11; 4097]).expect("the frame must be written");
        let mut reader = std::io::Cursor::new(encoded);
        assert!(matches!(
            crate::frame::read_frame(&mut reader, config.max_frame_bytes()),
            Err(crate::frame::FrameError::TooLarge {
                announced: 4097,
                max: 4096
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
