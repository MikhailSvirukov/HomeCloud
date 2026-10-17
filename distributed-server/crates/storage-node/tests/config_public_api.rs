//! Integration tests exercising only the public `storage_node` configuration API.

use std::path::PathBuf;

use storage_node::{ConfigError, NodeConfig, NodeId};

const NODE_ID: &str = "node-1";
const LISTEN_ADDR: &str = "0.0.0.0:9000";
const DATA_DIR: &str = "/var/lib/storage-node";
const ENCRYPTION_KEY_PATH: &str = "/keys/encryption.key";
const SIGNING_KEY_PATH: &str = "/keys/signing.key";
const KEY_NODE_ID: &str = "node_id";
const KEY_LISTEN_ADDR: &str = "listen_addr";
const KEY_DATA_DIR: &str = "data_dir";
const KEY_ENCRYPTION_KEY_PATH: &str = "encryption_key_path";
const KEY_SIGNING_KEY_PATH: &str = "signing_key_path";
const KEY_PEERS: &str = "peers";

#[test]
fn from_pairs_builds_config_with_comma_separated_peers_and_whitespace() {
    let config = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, " 0.0.0.0:9000 "),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_PEERS, " 10.0.0.2:9000 ,10.0.0.3:9000 "),
    ])
    .expect("valid pairs build a config");

    assert_eq!(config.node_id.as_str(), NODE_ID);
    assert_eq!(config.listen_addr, LISTEN_ADDR);
    assert_eq!(config.data_dir, PathBuf::from(DATA_DIR));
    assert_eq!(
        config.encryption_key_path,
        PathBuf::from(ENCRYPTION_KEY_PATH)
    );
    assert_eq!(config.signing_key_path, PathBuf::from(SIGNING_KEY_PATH));
    assert_eq!(config.peers, ["10.0.0.2:9000", "10.0.0.3:9000"]);
}

#[test]
fn from_pairs_without_node_id_reports_missing_key() {
    let result =
        NodeConfig::from_pairs(&[(KEY_LISTEN_ADDR, LISTEN_ADDR), (KEY_DATA_DIR, DATA_DIR)]);

    assert_eq!(result, Err(ConfigError::MissingKey { key: KEY_NODE_ID }));
}

#[test]
fn from_pairs_without_encryption_key_path_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_ENCRYPTION_KEY_PATH,
        })
    );
}

#[test]
fn from_pairs_without_signing_key_path_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_SIGNING_KEY_PATH,
        })
    );
}

#[test]
fn from_pairs_with_invalid_listen_addr_reports_invalid_address() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, "10.0.0.1"),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
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
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_PEERS, "10.0.0.2"),
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
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_PEERS, "10.0.0.2:9000,10.0.0.2:9000"),
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
