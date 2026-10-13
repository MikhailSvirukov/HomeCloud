//! Integration tests exercising only the public `storage_node` configuration API.

use std::path::PathBuf;

use storage_node::{ConfigError, NodeConfig, NodeId};

#[test]
fn from_pairs_builds_config_with_comma_separated_peers_and_whitespace() {
    let config = NodeConfig::from_pairs(&[
        ("node_id", "node-1"),
        ("listen_addr", " 0.0.0.0:9000 "),
        ("data_dir", "/var/lib/storage-node"),
        ("peers", " 10.0.0.2:9000 ,10.0.0.3:9000 "),
    ])
    .expect("valid pairs build a config");

    assert_eq!(config.node_id.as_str(), "node-1");
    assert_eq!(config.listen_addr, "0.0.0.0:9000");
    assert_eq!(config.data_dir, PathBuf::from("/var/lib/storage-node"));
    assert_eq!(config.peers, ["10.0.0.2:9000", "10.0.0.3:9000"]);
}

#[test]
fn from_pairs_without_node_id_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        ("listen_addr", "0.0.0.0:9000"),
        ("data_dir", "/var/lib/storage-node"),
    ]);

    assert_eq!(result, Err(ConfigError::MissingKey { key: "node_id" }));
}

#[test]
fn from_pairs_with_invalid_listen_addr_reports_invalid_address() {
    let result = NodeConfig::from_pairs(&[
        ("node_id", "node-1"),
        ("listen_addr", "10.0.0.1"),
        ("data_dir", "/var/lib/storage-node"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidListenAddr {
            value: "10.0.0.1".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_invalid_peer_addr_reports_invalid_peer_address() {
    let result = NodeConfig::from_pairs(&[
        ("node_id", "node-1"),
        ("listen_addr", "0.0.0.0:9000"),
        ("data_dir", "/var/lib/storage-node"),
        ("peers", "10.0.0.2"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidPeerAddr {
            value: "10.0.0.2".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_repeated_peer_reports_duplicate() {
    let result = NodeConfig::from_pairs(&[
        ("node_id", "node-1"),
        ("listen_addr", "0.0.0.0:9000"),
        ("data_dir", "/var/lib/storage-node"),
        ("peers", "10.0.0.2:9000,10.0.0.2:9000"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::DuplicatePeer {
            addr: "10.0.0.2:9000".to_owned(),
        })
    );
}

#[test]
fn node_id_parse_rejects_characters_outside_the_allowed_set() {
    assert_eq!(
        NodeId::parse("bad id"),
        Err(ConfigError::InvalidNodeIdCharacter { character: ' ' })
    );
}

#[test]
fn node_id_round_trips_through_display_and_from_str() {
    let id = NodeId::parse("node-01_A").expect("label uses allowed characters");

    assert_eq!(id.as_str(), "node-01_A");
    assert_eq!(id.to_string(), "node-01_A");
    assert_eq!("node-01_A".parse::<NodeId>(), Ok(id));
}
