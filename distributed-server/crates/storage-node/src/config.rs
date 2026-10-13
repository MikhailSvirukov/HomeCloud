//! Node identity and static configuration.
//!
//! A storage node has a stable [`NodeId`] label, a `host:port` listen address,
//! a data directory, and the addresses of its fixed peers. v1 clusters are
//! fixed, so this model is strictly validated and immutable once built.

use std::path::{Path, PathBuf};

/// Maximum accepted length of a node label in bytes.
pub const MAX_NODE_ID_LEN: usize = 63;

/// Recognized `from_pairs` key for the node label.
const KEY_NODE_ID: &str = "node_id";
/// Recognized `from_pairs` key for the listen address.
const KEY_LISTEN_ADDR: &str = "listen_addr";
/// Recognized `from_pairs` key for the data directory.
const KEY_DATA_DIR: &str = "data_dir";
/// Recognized `from_pairs` key for the peer address list.
const KEY_PEERS: &str = "peers";
/// Separator between peer addresses in the `peers` pair value.
const KEY_PEERS_SEPARATOR: char = ',';

/// Stable node ID/label.
///
/// Derived from a validated non-empty label of at most [`MAX_NODE_ID_LEN`]
/// ASCII alphanumeric, `-`, or `_` characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeId(String);

impl NodeId {
    /// Validates `label` and builds a [`NodeId`].
    ///
    /// # Errors
    ///
    /// [`ConfigError::EmptyNodeId`] for an empty label,
    /// [`ConfigError::NodeIdTooLong`] beyond [`MAX_NODE_ID_LEN`] bytes, or
    /// [`ConfigError::InvalidNodeIdCharacter`] for a character outside
    /// `[A-Za-z0-9_-]`.
    pub fn parse(label: &str) -> Result<Self, ConfigError> {
        if label.is_empty() {
            Err(ConfigError::EmptyNodeId)
        } else if label.len() > MAX_NODE_ID_LEN {
            Err(ConfigError::NodeIdTooLong {
                len: label.len(),
                max: MAX_NODE_ID_LEN,
            })
        } else if let Some(character) = label.chars().find(|c| !is_valid_node_id_char(*c)) {
            Err(ConfigError::InvalidNodeIdCharacter { character })
        } else {
            Ok(Self(label.to_owned()))
        }
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

    fn from_str(label: &str) -> Result<Self, Self::Err> {
        Self::parse(label)
    }
}

/// Validated configuration of a single storage node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    node_id: NodeId,
    listen_addr: String,
    data_dir: PathBuf,
    peers: Vec<String>,
}

impl NodeConfig {
    /// Validates and builds a configuration.
    ///
    /// `listen_addr` is whitespace-trimmed and must be `host:port` with a
    /// non-empty host and a port in `1..=65535`. Each peer is trimmed, must be
    /// a non-empty address passed through the same `host:port` rule, and must
    /// not repeat an earlier peer.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidListenAddr`] for a malformed address,
    /// [`ConfigError::EmptyPeerAddr`] for an empty peer, or
    /// [`ConfigError::DuplicatePeer`] for a repeated peer.
    pub fn new(
        node_id: NodeId,
        listen_addr: &str,
        data_dir: PathBuf,
        peers: Vec<String>,
    ) -> Result<Self, ConfigError> {
        let listen_addr = listen_addr.trim();
        validate_addr(listen_addr)?;
        Ok(Self {
            node_id,
            listen_addr: listen_addr.to_owned(),
            data_dir,
            peers: normalize_peers(peers)?,
        })
    }

    /// Builds a configuration from `key=value` pairs.
    ///
    /// Recognized keys are `node_id`, `listen_addr`, `data_dir`, and `peers`.
    /// The `peers` value is separated by commas and may be an empty string; a
    /// missing `peers` key means no peers. Unknown keys are ignored.
    ///
    /// # Errors
    ///
    /// [`ConfigError::MissingKey`] when `node_id`, `listen_addr`, or
    /// `data_dir` is absent or empty, or any error from [`NodeConfig::new`].
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Result<Self, ConfigError> {
        let mut node_id = None;
        let mut listen_addr = None;
        let mut data_dir = None;
        let mut peers = Vec::new();

        for (key, value) in pairs {
            match *key {
                KEY_NODE_ID if !value.is_empty() => node_id = Some(NodeId::parse(value)?),
                KEY_LISTEN_ADDR if !value.is_empty() => listen_addr = Some((*value).to_owned()),
                KEY_DATA_DIR if !value.is_empty() => data_dir = Some(PathBuf::from(value)),
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

        Self::new(node_id, &listen_addr, data_dir, peers)
    }

    /// Node identity label.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Trimmed `host:port` listen address.
    pub fn listen_addr(&self) -> &str {
        &self.listen_addr
    }

    /// Data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Trimmed, de-duplicated peer addresses.
    pub fn peers(&self) -> &[String] {
        &self.peers
    }
}

/// Failure modes when building a [`NodeId`] or [`NodeConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The node label was empty.
    EmptyNodeId,
    /// The node label exceeded [`MAX_NODE_ID_LEN`] bytes.
    NodeIdTooLong { len: usize, max: usize },
    /// The node label contained a character outside `[A-Za-z0-9_-]`.
    InvalidNodeIdCharacter { character: char },
    /// A required configuration key was missing or empty.
    MissingKey { key: &'static str },
    /// An address was not a valid `host:port`.
    InvalidListenAddr { value: String },
    /// A peer entry was empty after trimming.
    EmptyPeerAddr,
    /// The same peer address appeared more than once.
    DuplicatePeer { addr: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyNodeId => write!(formatter, "node id must not be empty"),
            Self::NodeIdTooLong { len, max } => {
                write!(formatter, "node id length {len} exceeds maximum {max}")
            }
            Self::InvalidNodeIdCharacter { character } => {
                write!(
                    formatter,
                    "node id contains invalid character {character:?}"
                )
            }
            Self::MissingKey { key } => write!(formatter, "missing required key {key}"),
            Self::InvalidListenAddr { value } => write!(formatter, "invalid address {value}"),
            Self::EmptyPeerAddr => write!(formatter, "peer address must not be empty"),
            Self::DuplicatePeer { addr } => write!(formatter, "duplicate peer address {addr}"),
        }
    }
}

impl std::error::Error for ConfigError {}

fn is_valid_node_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

fn validate_addr(addr: &str) -> Result<(), ConfigError> {
    match addr.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && is_valid_port(port) => Ok(()),
        _ => Err(ConfigError::InvalidListenAddr {
            value: addr.to_owned(),
        }),
    }
}

fn is_valid_port(port: &str) -> bool {
    matches!(port.parse::<u16>(), Ok(value) if value >= 1)
}

fn normalize_peers(peers: Vec<String>) -> Result<Vec<String>, ConfigError> {
    let mut normalized: Vec<String> = Vec::with_capacity(peers.len());
    for peer in peers {
        let trimmed = peer.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::EmptyPeerAddr);
        }
        validate_addr(trimmed)?;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        ConfigError, NodeConfig, NodeId, KEY_DATA_DIR, KEY_LISTEN_ADDR, KEY_NODE_ID,
        MAX_NODE_ID_LEN,
    };

    fn node_id(label: &str) -> NodeId {
        NodeId::parse(label).expect("test label is valid")
    }

    fn data_dir() -> PathBuf {
        PathBuf::from("/data")
    }

    fn invalid_addr(value: &str) -> ConfigError {
        ConfigError::InvalidListenAddr {
            value: value.to_owned(),
        }
    }

    #[test]
    fn parse_accepts_valid_label_and_round_trips_via_display_and_from_str() {
        let id = node_id("node-01_A");
        assert_eq!(id.as_str(), "node-01_A");
        assert_eq!(id.to_string(), "node-01_A");
        assert_eq!("node-01_A".parse::<NodeId>(), Ok(id));
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
    fn parse_rejects_invalid_character() {
        let expected = ConfigError::InvalidNodeIdCharacter { character: ' ' };
        assert_eq!(NodeId::parse("node 01"), Err(expected));
    }

    #[test]
    fn new_accepts_valid_config_with_trimmed_peers() {
        let config = NodeConfig::new(
            node_id("node-1"),
            " 0.0.0.0:9000 ",
            data_dir(),
            vec![" 10.0.0.2:9000 ".to_owned()],
        )
        .expect("config is valid");

        assert_eq!(config.node_id().as_str(), "node-1");
        assert_eq!(config.listen_addr(), "0.0.0.0:9000");
        assert_eq!(config.data_dir(), PathBuf::from("/data"));
        assert_eq!(config.peers(), ["10.0.0.2:9000"]);
    }

    #[test]
    fn new_rejects_address_without_port() {
        let result = NodeConfig::new(node_id("node-1"), "10.0.0.1", data_dir(), Vec::new());
        assert_eq!(result, Err(invalid_addr("10.0.0.1")));
    }

    #[test]
    fn new_rejects_non_numeric_port() {
        let result = NodeConfig::new(node_id("node-1"), "10.0.0.1:http", data_dir(), Vec::new());
        assert_eq!(result, Err(invalid_addr("10.0.0.1:http")));
    }

    #[test]
    fn new_rejects_port_zero() {
        let result = NodeConfig::new(node_id("node-1"), "10.0.0.1:0", data_dir(), Vec::new());
        assert_eq!(result, Err(invalid_addr("10.0.0.1:0")));
    }

    #[test]
    fn new_rejects_empty_peer() {
        let result = NodeConfig::new(
            node_id("node-1"),
            "10.0.0.1:9000",
            data_dir(),
            vec!["   ".to_owned()],
        );
        assert_eq!(result, Err(ConfigError::EmptyPeerAddr));
    }

    #[test]
    fn new_rejects_duplicate_peer() {
        let result = NodeConfig::new(
            node_id("node-1"),
            "10.0.0.1:9000",
            data_dir(),
            vec!["10.0.0.2:9000".to_owned(), "10.0.0.2:9000".to_owned()],
        );
        let expected = ConfigError::DuplicatePeer {
            addr: "10.0.0.2:9000".to_owned(),
        };
        assert_eq!(result, Err(expected));
    }

    #[test]
    fn from_pairs_splits_and_trims_comma_separated_peers() {
        let config = NodeConfig::from_pairs(&[
            ("node_id", "node-1"),
            ("listen_addr", " 0.0.0.0:9000 "),
            ("data_dir", "/data"),
            ("peers", " 10.0.0.2:9000 , 10.0.0.3:9000 "),
        ])
        .expect("pairs are valid");

        assert_eq!(config.listen_addr(), "0.0.0.0:9000");
        assert_eq!(config.peers(), ["10.0.0.2:9000", "10.0.0.3:9000"]);
    }

    #[test]
    fn from_pairs_with_empty_peer_value_has_no_peers() {
        let config = NodeConfig::from_pairs(&[
            ("node_id", "node-1"),
            ("listen_addr", "0.0.0.0:9000"),
            ("data_dir", "/data"),
            ("peers", ""),
        ])
        .expect("pairs are valid");

        assert!(config.peers().is_empty());
    }

    #[test]
    fn from_pairs_requires_node_id() {
        let pairs = [("listen_addr", "0.0.0.0:9000"), ("data_dir", "/data")];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey { key: KEY_NODE_ID })
        );
    }

    #[test]
    fn from_pairs_requires_listen_addr() {
        let pairs = [("node_id", "node-1"), ("data_dir", "/data")];
        assert_eq!(
            NodeConfig::from_pairs(&pairs),
            Err(ConfigError::MissingKey {
                key: KEY_LISTEN_ADDR,
            })
        );
    }

    #[test]
    fn from_pairs_requires_data_dir() {
        let pairs = [("node_id", "node-1"), ("listen_addr", "0.0.0.0:9000")];
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
        ])
        .expect("unknown keys are ignored");

        assert_eq!(config.node_id().as_str(), "node-1");
    }
}
