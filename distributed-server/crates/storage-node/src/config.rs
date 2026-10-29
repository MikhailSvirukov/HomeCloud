//! Node identity and static configuration.
//!
//! A storage node has a stable [`NodeId`] label, a `host:port` listen address,
//! a data directory, the paths of its encryption and signing key files, the
//! maximum frame length its framing layer accepts, the per-connection queue
//! capacity, the catalog page size and snapshot lifetime, the source-name byte
//! limit, and the addresses of its fixed peers. v1 clusters are fixed, so this
//! model is validated when it is built.

use core::num::{NonZeroU16, NonZeroU32, NonZeroU64, NonZeroUsize};
use std::path::PathBuf;

/// Fallible configuration operation result.
type Result<T> = core::result::Result<T, ConfigError>;

/// Maximum accepted length of a node label in bytes.
pub const MAX_NODE_ID_LEN: usize = 63;

/// Recognized `from_pairs` key for the node label.
const KEY_NODE_ID: &str = "node_id";
/// Recognized `from_pairs` key for the listen address.
const KEY_LISTEN_ADDR: &str = "listen_addr";
/// Recognized `from_pairs` key for the data directory.
const KEY_DATA_DIR: &str = "data_dir";
/// Recognized `from_pairs` key for the encryption key file path.
const KEY_ENCRYPTION_KEY_PATH: &str = "encryption_key_path";
/// Recognized `from_pairs` key for the signing key file path.
const KEY_SIGNING_KEY_PATH: &str = "signing_key_path";
/// Recognized `from_pairs` key for the maximum frame length.
const KEY_MAX_FRAME_LEN: &str = "max_frame_len";
/// Recognized `from_pairs` key for the per-connection queue capacity.
const KEY_QUEUE_CAPACITY: &str = "queue_capacity";
/// Recognized `from_pairs` key for the catalog page size.
const KEY_CATALOG_PAGE_SIZE: &str = "catalog_page_size";
/// Recognized `from_pairs` key for the catalog snapshot lifetime in milliseconds.
const KEY_CATALOG_SNAPSHOT_TTL_MS: &str = "catalog_snapshot_ttl_ms";
/// Recognized `from_pairs` key for the maximum source-name length in bytes.
const KEY_MAX_SOURCE_NAME_LEN: &str = "max_source_name_len";
/// Recognized `from_pairs` key for the peer address list.
const KEY_PEERS: &str = "peers";
/// Separator between peer addresses in the `peers` pair value.
const KEY_PEERS_SEPARATOR: char = ',';

/// Stable node ID/label derived from a validated ASCII label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeId(String);

impl NodeId {
    /// Validates `label` and builds a [`NodeId`].
    pub fn parse(label: &str) -> Result<Self> {
        if label.is_empty() {
            return Err(ConfigError::EmptyNodeId);
        }
        if label.len() > MAX_NODE_ID_LEN {
            return Err(ConfigError::NodeIdTooLong {
                len: label.len(),
                max: MAX_NODE_ID_LEN,
            });
        }
        if let Some(character) = label.chars().find(|c| !is_valid_node_id_char(*c)) {
            return Err(ConfigError::InvalidNodeIdCharacter { character });
        }
        Ok(Self(label.to_owned()))
    }

    /// Returns the label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::str::FromStr for NodeId {
    type Err = ConfigError;

    fn from_str(label: &str) -> Result<Self> {
        Self::parse(label)
    }
}

/// Validated configuration of a single storage node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    pub node_id: NodeId,
    pub listen_addr: String,
    pub data_dir: PathBuf,
    pub encryption_key_path: PathBuf,
    pub signing_key_path: PathBuf,
    pub max_frame_len: NonZeroU32,
    pub queue_capacity: NonZeroUsize,
    pub catalog_page_size: NonZeroUsize,
    pub catalog_snapshot_ttl_ms: NonZeroU64,
    pub max_source_name_len: NonZeroUsize,
    pub peers: Vec<String>,
}

impl NodeConfig {
    /// Validates and builds a configuration.
    #[expect(
        clippy::too_many_arguments,
        reason = "the constructor takes the complete node configuration surface"
    )]
    pub fn new(
        node_id: NodeId,
        listen_addr: &str,
        data_dir: PathBuf,
        encryption_key_path: PathBuf,
        signing_key_path: PathBuf,
        max_frame_len: NonZeroU32,
        queue_capacity: NonZeroUsize,
        catalog_page_size: NonZeroUsize,
        catalog_snapshot_ttl_ms: NonZeroU64,
        max_source_name_len: NonZeroUsize,
        peers: Vec<String>,
    ) -> Result<Self> {
        let listen_addr = listen_addr.trim();
        validate_listen_addr(listen_addr)?;
        Ok(Self {
            node_id,
            listen_addr: listen_addr.to_owned(),
            data_dir,
            encryption_key_path,
            signing_key_path,
            max_frame_len,
            queue_capacity,
            catalog_page_size,
            catalog_snapshot_ttl_ms,
            max_source_name_len,
            peers: normalize_peers(peers)?,
        })
    }

    /// Builds a catalog service with this node's configured page size, snapshot lifetime, and source-name limit.
    pub fn catalog_service(&self) -> crate::catalog_service::CatalogService {
        crate::catalog_service::CatalogService::new(
            self.catalog_page_size,
            core::time::Duration::from_millis(self.catalog_snapshot_ttl_ms.get()),
            self.max_source_name_len,
        )
    }

    /// Builds a configuration from `node_id`, `listen_addr`, `data_dir`, key path, frame limit, queue capacity, catalog settings, and `peers` pairs.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Result<Self> {
        let mut node_id = None;
        let mut listen_addr = None;
        let mut data_dir = None;
        let mut encryption_key_path = None;
        let mut signing_key_path = None;
        let mut max_frame_len = None;
        let mut queue_capacity = None;
        let mut catalog_page_size = None;
        let mut catalog_snapshot_ttl_ms = None;
        let mut max_source_name_len = None;
        let mut peers = Vec::new();

        for (key, value) in pairs {
            match *key {
                KEY_NODE_ID if !value.is_empty() => node_id = Some(NodeId::parse(value)?),
                KEY_LISTEN_ADDR if !value.is_empty() => listen_addr = Some((*value).to_owned()),
                KEY_DATA_DIR if !value.is_empty() => data_dir = Some(PathBuf::from(value)),
                KEY_ENCRYPTION_KEY_PATH if !value.is_empty() => {
                    encryption_key_path = Some(PathBuf::from(value));
                }
                KEY_SIGNING_KEY_PATH if !value.is_empty() => {
                    signing_key_path = Some(PathBuf::from(value));
                }
                KEY_MAX_FRAME_LEN if !value.is_empty() => max_frame_len = Some(*value),
                KEY_QUEUE_CAPACITY if !value.is_empty() => queue_capacity = Some(*value),
                KEY_CATALOG_PAGE_SIZE if !value.is_empty() => catalog_page_size = Some(*value),
                KEY_CATALOG_SNAPSHOT_TTL_MS if !value.is_empty() => {
                    catalog_snapshot_ttl_ms = Some(*value);
                }
                KEY_MAX_SOURCE_NAME_LEN if !value.is_empty() => {
                    max_source_name_len = Some(*value);
                }
                KEY_PEERS => peers = split_peers(value),
                _ => {}
            }
        }

        let Some(node_id) = node_id else {
            return Err(ConfigError::MissingKey { key: KEY_NODE_ID });
        };
        let Some(listen_addr) = listen_addr else {
            return Err(ConfigError::MissingKey {
                key: KEY_LISTEN_ADDR,
            });
        };
        let Some(data_dir) = data_dir else {
            return Err(ConfigError::MissingKey { key: KEY_DATA_DIR });
        };
        let Some(encryption_key_path) = encryption_key_path else {
            return Err(ConfigError::MissingKey {
                key: KEY_ENCRYPTION_KEY_PATH,
            });
        };
        let Some(signing_key_path) = signing_key_path else {
            return Err(ConfigError::MissingKey {
                key: KEY_SIGNING_KEY_PATH,
            });
        };
        let Some(max_frame_len) = max_frame_len else {
            return Err(ConfigError::MissingKey {
                key: KEY_MAX_FRAME_LEN,
            });
        };
        let max_frame_len = parse_max_frame_len(max_frame_len)?;
        let Some(queue_capacity) = queue_capacity else {
            return Err(ConfigError::MissingKey {
                key: KEY_QUEUE_CAPACITY,
            });
        };
        let queue_capacity = parse_queue_capacity(queue_capacity)?;
        let Some(catalog_page_size) = catalog_page_size else {
            return Err(ConfigError::MissingKey {
                key: KEY_CATALOG_PAGE_SIZE,
            });
        };
        let catalog_page_size = parse_catalog_page_size(catalog_page_size)?;
        let Some(catalog_snapshot_ttl_ms) = catalog_snapshot_ttl_ms else {
            return Err(ConfigError::MissingKey {
                key: KEY_CATALOG_SNAPSHOT_TTL_MS,
            });
        };
        let catalog_snapshot_ttl_ms = parse_catalog_snapshot_ttl(catalog_snapshot_ttl_ms)?;
        let Some(max_source_name_len) = max_source_name_len else {
            return Err(ConfigError::MissingKey {
                key: KEY_MAX_SOURCE_NAME_LEN,
            });
        };
        let max_source_name_len = parse_max_source_name_len(max_source_name_len)?;

        Self::new(
            node_id,
            &listen_addr,
            data_dir,
            encryption_key_path,
            signing_key_path,
            max_frame_len,
            queue_capacity,
            catalog_page_size,
            catalog_snapshot_ttl_ms,
            max_source_name_len,
            peers,
        )
    }
}

/// Failure modes when building a [`NodeId`] or [`NodeConfig`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The node label was empty.
    #[error("node id must not be empty")]
    EmptyNodeId,
    /// The node label exceeded [`MAX_NODE_ID_LEN`] bytes.
    #[error("node id length {len} exceeds maximum {max}")]
    NodeIdTooLong { len: usize, max: usize },
    /// The node label contained a character outside `[A-Za-z0-9_-]`.
    #[error("node id contains invalid character {character:?}")]
    InvalidNodeIdCharacter { character: char },
    /// A required configuration key was missing or empty.
    #[error("missing required key {key}")]
    MissingKey { key: &'static str },
    /// The listen address was not a valid `host:port`.
    #[error("invalid address {value}")]
    InvalidListenAddr { value: String },
    /// A peer address was not a valid `host:port`.
    #[error("invalid peer address {value}")]
    InvalidPeerAddr { value: String },
    /// A peer entry was empty after trimming.
    #[error("peer address must not be empty")]
    EmptyPeerAddr,
    /// The maximum frame length was not a positive integer.
    #[error("invalid maximum frame length {value}")]
    InvalidMaxFrameLen { value: String },
    /// The queue capacity was zero or not an integer.
    #[error("invalid queue capacity {value}")]
    InvalidQueueCapacity { value: String },
    /// The catalog page size was zero or not an integer.
    #[error("invalid catalog page size {value}")]
    InvalidCatalogPageSize { value: String },
    /// The catalog snapshot TTL was zero or not an integer.
    #[error("invalid catalog snapshot TTL {value}")]
    InvalidCatalogSnapshotTtl { value: String },
    /// The maximum source-name length was zero or not an integer.
    #[error("invalid maximum source name length {value}")]
    InvalidMaxSourceNameLen { value: String },
    /// The same peer address appeared more than once.
    #[error("duplicate peer address {addr}")]
    DuplicatePeer { addr: String },
}

fn is_valid_node_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

/// Returns whether `addr` is a non-empty `host:port`.
fn is_valid_host_port(addr: &str) -> bool {
    match addr.rsplit_once(':') {
        Some((host, port)) => !host.is_empty() && is_valid_port(port),
        None => false,
    }
}

/// Validates a listen address.
fn validate_listen_addr(addr: &str) -> Result<()> {
    if !is_valid_host_port(addr) {
        return Err(ConfigError::InvalidListenAddr {
            value: addr.to_owned(),
        });
    }
    Ok(())
}

/// Validates a peer address.
fn validate_peer_addr(addr: &str) -> Result<()> {
    if !is_valid_host_port(addr) {
        return Err(ConfigError::InvalidPeerAddr {
            value: addr.to_owned(),
        });
    }
    Ok(())
}

fn is_valid_port(port: &str) -> bool {
    port.parse::<NonZeroU16>().is_ok()
}

fn normalize_peers(peers: Vec<String>) -> Result<Vec<String>> {
    let mut normalized: Vec<String> = Vec::with_capacity(peers.len());
    for peer in peers {
        let trimmed = peer.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::EmptyPeerAddr);
        }
        validate_peer_addr(trimmed)?;
        if normalized.iter().any(|existing| existing == trimmed) {
            return Err(ConfigError::DuplicatePeer {
                addr: trimmed.to_owned(),
            });
        }
        normalized.push(trimmed.to_owned());
    }
    Ok(normalized)
}

fn split_peers(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value
            .split(KEY_PEERS_SEPARATOR)
            .map(str::to_owned)
            .collect()
    }
}

/// Parses a maximum frame length that must be a non-zero `u32`.
fn parse_max_frame_len(value: &str) -> Result<NonZeroU32> {
    value
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| ConfigError::InvalidMaxFrameLen {
            value: value.to_owned(),
        })
}

/// Parses a queue capacity that must be a non-zero `usize`.
fn parse_queue_capacity(value: &str) -> Result<NonZeroUsize> {
    value
        .parse::<usize>()
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| ConfigError::InvalidQueueCapacity {
            value: value.to_owned(),
        })
}

/// Parses a catalog page size that must be a non-zero `usize`.
fn parse_catalog_page_size(value: &str) -> Result<NonZeroUsize> {
    value
        .parse::<usize>()
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| ConfigError::InvalidCatalogPageSize {
            value: value.to_owned(),
        })
}

/// Parses a catalog snapshot lifetime that must be a non-zero `u64` of milliseconds.
fn parse_catalog_snapshot_ttl(value: &str) -> Result<NonZeroU64> {
    value
        .parse::<u64>()
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(|| ConfigError::InvalidCatalogSnapshotTtl {
            value: value.to_owned(),
        })
}

/// Parses a maximum source-name length that must be a non-zero `usize`.
fn parse_max_source_name_len(value: &str) -> Result<NonZeroUsize> {
    value
        .parse::<usize>()
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| ConfigError::InvalidMaxSourceNameLen {
            value: value.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
    use std::path::PathBuf;

    use super::{
        ConfigError, NodeConfig, NodeId, KEY_CATALOG_PAGE_SIZE, KEY_CATALOG_SNAPSHOT_TTL_MS,
        KEY_DATA_DIR, KEY_ENCRYPTION_KEY_PATH, KEY_LISTEN_ADDR, KEY_MAX_FRAME_LEN,
        KEY_MAX_SOURCE_NAME_LEN, KEY_NODE_ID, KEY_QUEUE_CAPACITY, KEY_SIGNING_KEY_PATH,
        MAX_NODE_ID_LEN,
    };

    const ENCRYPTION_KEY_PATH: &str = "/keys/encryption.key";
    const SIGNING_KEY_PATH: &str = "/keys/signing.key";

    fn node_id(label: &str) -> NodeId {
        NodeId::parse(label).expect("test label is valid")
    }

    fn max_frame_len() -> NonZeroU32 {
        NonZeroU32::new(1_048_576).expect("test limit is non-zero")
    }

    fn queue_capacity() -> NonZeroUsize {
        NonZeroUsize::new(16).expect("test capacity is non-zero")
    }

    fn catalog_page_size() -> NonZeroUsize {
        NonZeroUsize::new(50).expect("test page size is non-zero")
    }

    fn catalog_snapshot_ttl_ms() -> NonZeroU64 {
        NonZeroU64::new(30_000).expect("test snapshot TTL is non-zero")
    }

    fn max_source_name_len() -> NonZeroUsize {
        NonZeroUsize::new(255).expect("test name limit is non-zero")
    }

    fn data_dir() -> PathBuf {
        PathBuf::from("/data")
    }

    fn encryption_key_path() -> PathBuf {
        PathBuf::from(ENCRYPTION_KEY_PATH)
    }

    fn signing_key_path() -> PathBuf {
        PathBuf::from(SIGNING_KEY_PATH)
    }

    #[test]
    fn parse_accepts_label_of_maximum_length() {
        assert!(NodeId::parse(&"a".repeat(MAX_NODE_ID_LEN)).is_ok());
    }

    #[test]
    fn parse_rejects_empty_label() {
        assert_eq!(NodeId::parse(""), Err(ConfigError::EmptyNodeId));
    }

    #[test]
    fn parse_rejects_label_over_maximum_length() {
        let over = "a".repeat(MAX_NODE_ID_LEN + 1);
        let expected = ConfigError::NodeIdTooLong {
            len: MAX_NODE_ID_LEN + 1,
            max: MAX_NODE_ID_LEN,
        };
        assert_eq!(NodeId::parse(&over), Err(expected));
    }

    #[test]
    fn new_accepts_valid_config_with_trimmed_peers() {
        let config = NodeConfig::new(
            node_id("node-1"),
            " 0.0.0.0:9000 ",
            data_dir(),
            encryption_key_path(),
            signing_key_path(),
            max_frame_len(),
            queue_capacity(),
            catalog_page_size(),
            catalog_snapshot_ttl_ms(),
            max_source_name_len(),
            vec![" 10.0.0.2:9000 ".to_owned()],
        )
        .expect("config is valid");

        assert_eq!(config.node_id.as_str(), "node-1");
        assert_eq!(config.listen_addr, "0.0.0.0:9000");
        assert_eq!(config.data_dir, PathBuf::from("/data"));
        assert_eq!(config.encryption_key_path, encryption_key_path());
        assert_eq!(config.signing_key_path, signing_key_path());
        assert_eq!(config.max_frame_len.get(), 1_048_576);
        assert_eq!(config.queue_capacity.get(), 16);
        assert_eq!(config.peers, ["10.0.0.2:9000"]);
    }

    #[test]
    fn new_rejects_invalid_listen_addresses() {
        for value in ["10.0.0.1", "10.0.0.1:http", "10.0.0.1:0"] {
            let result = NodeConfig::new(
                node_id("node-1"),
                value,
                data_dir(),
                encryption_key_path(),
                signing_key_path(),
                max_frame_len(),
                queue_capacity(),
                catalog_page_size(),
                catalog_snapshot_ttl_ms(),
                max_source_name_len(),
                Vec::new(),
            );
            let expected = ConfigError::InvalidListenAddr {
                value: value.to_owned(),
            };
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn new_rejects_empty_peer() {
        let result = NodeConfig::new(
            node_id("node-1"),
            "10.0.0.1:9000",
            data_dir(),
            encryption_key_path(),
            signing_key_path(),
            max_frame_len(),
            queue_capacity(),
            catalog_page_size(),
            catalog_snapshot_ttl_ms(),
            max_source_name_len(),
            vec!["   ".to_owned()],
        );
        assert_eq!(result, Err(ConfigError::EmptyPeerAddr));
    }

    #[test]
    fn new_rejects_invalid_peer_addr() {
        let result = NodeConfig::new(
            node_id("node-1"),
            "10.0.0.1:9000",
            data_dir(),
            encryption_key_path(),
            signing_key_path(),
            max_frame_len(),
            queue_capacity(),
            catalog_page_size(),
            catalog_snapshot_ttl_ms(),
            max_source_name_len(),
            vec!["10.0.0.2".to_owned()],
        );
        let expected = ConfigError::InvalidPeerAddr {
            value: "10.0.0.2".to_owned(),
        };
        assert_eq!(result, Err(expected));
    }

    #[test]
    fn from_pairs_with_empty_peer_value_has_no_peers() {
        let config = NodeConfig::from_pairs(&[
            ("node_id", "node-1"),
            ("listen_addr", "0.0.0.0:9000"),
            ("data_dir", "/data"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
            (KEY_MAX_FRAME_LEN, "1048576"),
            (KEY_QUEUE_CAPACITY, "16"),
            (KEY_CATALOG_PAGE_SIZE, "50"),
            (KEY_CATALOG_SNAPSHOT_TTL_MS, "30000"),
            (KEY_MAX_SOURCE_NAME_LEN, "255"),
            ("peers", ""),
        ])
        .expect("pairs are valid");

        assert!(config.peers.is_empty());
    }

    #[test]
    fn from_pairs_requires_listen_addr() {
        let pairs = [
            ("node_id", "node-1"),
            ("data_dir", "/data"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        ];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey {
                key: KEY_LISTEN_ADDR,
            })
        );
    }

    #[test]
    fn from_pairs_requires_data_dir() {
        let pairs = [
            ("node_id", "node-1"),
            ("listen_addr", "0.0.0.0:9000"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        ];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey { key: KEY_DATA_DIR })
        );
    }

    #[test]
    fn from_pairs_treats_empty_data_dir_as_missing() {
        let pairs = [
            ("node_id", "node-1"),
            ("listen_addr", "0.0.0.0:9000"),
            ("data_dir", ""),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        ];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey { key: KEY_DATA_DIR })
        );
    }

    #[test]
    fn from_pairs_treats_empty_node_id_as_missing() {
        let pairs = [
            ("node_id", ""),
            ("listen_addr", "0.0.0.0:9000"),
            ("data_dir", "/data"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        ];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey { key: KEY_NODE_ID })
        );
    }

    #[test]
    fn from_pairs_treats_empty_listen_addr_as_missing() {
        let pairs = [
            ("node_id", "node-1"),
            ("listen_addr", ""),
            ("data_dir", "/data"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        ];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey {
                key: KEY_LISTEN_ADDR,
            })
        );
    }

    #[test]
    fn from_pairs_ignores_unknown_keys() {
        let config = NodeConfig::from_pairs(&[
            ("unknown", "ignored"),
            ("node_id", "node-1"),
            ("listen_addr", "0.0.0.0:9000"),
            ("data_dir", "/data"),
            (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
            (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
            (KEY_MAX_FRAME_LEN, "1048576"),
            (KEY_QUEUE_CAPACITY, "16"),
            (KEY_CATALOG_PAGE_SIZE, "50"),
            (KEY_CATALOG_SNAPSHOT_TTL_MS, "30000"),
            (KEY_MAX_SOURCE_NAME_LEN, "255"),
        ])
        .expect("unknown keys are ignored");

        assert_eq!(config.node_id.as_str(), "node-1");
    }
}
