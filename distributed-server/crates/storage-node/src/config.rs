//! Node identity and static configuration.
//!
//! A storage node has a stable [`NodeId`] label, a `host:port` listen address,
//! a data directory, and the addresses of its fixed peers. v1 clusters are
//! fixed, so this model is validated when it is built.

use core::num::NonZeroU16;
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
    pub peers: Vec<String>,
}

impl NodeConfig {
    /// Validates and builds a configuration.
    pub fn new(
        node_id: NodeId,
        listen_addr: &str,
        data_dir: PathBuf,
        peers: Vec<String>,
    ) -> Result<Self> {
        let listen_addr = listen_addr.trim();
        validate_listen_addr(listen_addr)?;
        Ok(Self {
            node_id,
            listen_addr: listen_addr.to_owned(),
            data_dir,
            peers: normalize_peers(peers)?,
        })
    }

    /// Builds a configuration from `node_id`, `listen_addr`, `data_dir`, and `peers` pairs.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Result<Self> {
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
            vec![" 10.0.0.2:9000 ".to_owned()],
        )
        .expect("config is valid");

        assert_eq!(config.node_id.as_str(), "node-1");
        assert_eq!(config.listen_addr, "0.0.0.0:9000");
        assert_eq!(config.data_dir, PathBuf::from("/data"));
        assert_eq!(config.peers, ["10.0.0.2:9000"]);
    }

    #[test]
    fn new_rejects_invalid_listen_addresses() {
        for value in ["10.0.0.1", "10.0.0.1:http", "10.0.0.1:0"] {
            let result = NodeConfig::new(node_id("node-1"), value, data_dir(), Vec::new());
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
            ("peers", ""),
        ])
        .expect("pairs are valid");

        assert!(config.peers.is_empty());
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

        assert_eq!(config.node_id.as_str(), "node-1");
    }
}
