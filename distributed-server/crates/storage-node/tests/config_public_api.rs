//! Integration tests exercising only the public `storage_node` configuration API.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use storage_node::{ConfigError, LogConfigError, NodeConfig, NodeId};

const NODE_ID: &str = "node-1";
const LISTEN_ADDR: &str = "0.0.0.0:9000";
const METRICS_BIND_ADDR: &str = "127.0.0.1:9100";
const DATA_DIR: &str = "/var/lib/storage-node";
const ENCRYPTION_KEY_PATH: &str = "/keys/encryption.key";
const SIGNING_KEY_PATH: &str = "/keys/signing.key";
const KEY_NODE_ID: &str = "node_id";
const KEY_LISTEN_ADDR: &str = "listen_addr";
const KEY_METRICS_BIND_ADDR: &str = "metrics_bind_addr";
const KEY_DATA_DIR: &str = "data_dir";
const KEY_ENCRYPTION_KEY_PATH: &str = "encryption_key_path";
const KEY_SIGNING_KEY_PATH: &str = "signing_key_path";
const KEY_MAX_FRAME_LEN: &str = "max_frame_len";
const KEY_QUEUE_CAPACITY: &str = "queue_capacity";
const KEY_PEERS: &str = "peers";
const KEY_CATALOG_PAGE_SIZE: &str = "catalog_page_size";
const KEY_CATALOG_SNAPSHOT_TTL_MS: &str = "catalog_snapshot_ttl_ms";
const KEY_MAX_SOURCE_NAME_LEN: &str = "max_source_name_len";
const KEY_LOG_FILE_PATH: &str = "log_file_path";
const KEY_LOG_QUEUE_CAPACITY: &str = "log_queue_capacity";
const KEY_LOG_MAX_FILE_BYTES: &str = "log_max_file_bytes";
const KEY_LOG_MAX_TOTAL_BYTES: &str = "log_max_total_bytes";
const KEY_LOG_MAX_FILE_AGE_SECONDS: &str = "log_max_file_age_seconds";
const MAX_FRAME_LEN: &str = "1048576";
const QUEUE_CAPACITY: &str = "16";
const CATALOG_PAGE_SIZE: &str = "2";
const CATALOG_SNAPSHOT_TTL_MS: &str = "60000";
const MAX_SOURCE_NAME_LEN: &str = "64";
const LOG_FILE_PATH: &str = "/var/log/storage-node/events.jsonl";
const LOG_QUEUE_CAPACITY: &str = "64";
const LOG_MAX_FILE_BYTES: &str = "1048576";
const LOG_MAX_TOTAL_BYTES: &str = "16777216";
const LOG_MAX_FILE_AGE_SECONDS: &str = "86400";

/// Complete valid `from_pairs` input including every required log setting.
fn valid_pairs() -> Vec<(&'static str, &'static str)> {
    vec![
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_PEERS, ""),
    ]
}

/// Returns the valid pairs without `key`, leaving every other required key present.
fn pairs_without(key: &str) -> Vec<(&'static str, &'static str)> {
    valid_pairs()
        .into_iter()
        .filter(|(pair_key, _)| *pair_key != key)
        .collect()
}

/// Returns the valid pairs with `key` replaced by `value`.
fn pairs_with(key: &str, value: &'static str) -> Vec<(&'static str, &'static str)> {
    valid_pairs()
        .into_iter()
        .map(|(pair_key, pair_value)| {
            if pair_key == key {
                (pair_key, value)
            } else {
                (pair_key, pair_value)
            }
        })
        .collect()
}

#[test]
fn from_pairs_builds_config_with_comma_separated_peers_and_whitespace() {
    let config = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, " 0.0.0.0:9000 "),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
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
    assert_eq!(config.max_frame_len.get(), 1_048_576);
    assert_eq!(config.queue_capacity.get(), 16);
    assert_eq!(config.log.path, PathBuf::from(LOG_FILE_PATH));
    assert_eq!(config.log.queue_capacity.get(), 64);
    assert_eq!(config.log.max_file_bytes.get(), 1_048_576);
    assert_eq!(config.log.max_total_bytes.get(), 16_777_216);
    assert_eq!(config.log.max_file_age, Duration::from_secs(86_400));
    assert_eq!(config.catalog_page_size.get(), 2);
    assert_eq!(config.catalog_snapshot_ttl_ms.get(), 60_000);
    assert_eq!(config.max_source_name_len.get(), 64);
    assert_eq!(
        config.metrics_bind_addr,
        SocketAddr::from(([127, 0, 0, 1], 9100))
    );
    assert_eq!(config.peers, ["10.0.0.2:9000", "10.0.0.3:9000"]);
}

#[test]
fn from_pairs_without_node_id_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(result, Err(ConfigError::MissingKey { key: KEY_NODE_ID }));
}

#[test]
fn from_pairs_without_encryption_key_path_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
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
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_SIGNING_KEY_PATH,
        })
    );
}

#[test]
fn from_pairs_without_max_frame_len_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_MAX_FRAME_LEN,
        })
    );
}

#[test]
fn from_pairs_with_zero_max_frame_len_reports_invalid_max_frame_len() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, "0"),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidMaxFrameLen {
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_max_frame_len_reports_invalid_max_frame_len() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, "not-a-number"),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidMaxFrameLen {
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_without_queue_capacity_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_QUEUE_CAPACITY,
        })
    );
}

#[test]
fn from_pairs_with_zero_queue_capacity_reports_invalid_queue_capacity() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, "0"),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidQueueCapacity {
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_queue_capacity_reports_invalid_queue_capacity() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, "not-a-number"),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidQueueCapacity {
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_without_catalog_page_size_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_CATALOG_PAGE_SIZE,
        })
    );
}

#[test]
fn from_pairs_with_zero_catalog_page_size_reports_invalid_catalog_page_size() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_CATALOG_PAGE_SIZE, "0"),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidCatalogPageSize {
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_catalog_page_size_reports_invalid_catalog_page_size() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_CATALOG_PAGE_SIZE, "not-a-number"),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidCatalogPageSize {
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_without_catalog_snapshot_ttl_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_CATALOG_SNAPSHOT_TTL_MS,
        })
    );
}

#[test]
fn from_pairs_with_zero_catalog_snapshot_ttl_reports_invalid_catalog_snapshot_ttl() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, "0"),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidCatalogSnapshotTtl {
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_catalog_snapshot_ttl_reports_invalid_catalog_snapshot_ttl() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, "not-a-number"),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidCatalogSnapshotTtl {
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_without_max_source_name_len_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_MAX_SOURCE_NAME_LEN,
        })
    );
}

#[test]
fn from_pairs_with_zero_max_source_name_len_reports_invalid_max_source_name_len() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, "0"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidMaxSourceNameLen {
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_max_source_name_len_reports_invalid_max_source_name_len() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, "not-a-number"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidMaxSourceNameLen {
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_invalid_listen_addr_reports_invalid_address() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, "10.0.0.1"),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
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
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
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
        (KEY_METRICS_BIND_ADDR, METRICS_BIND_ADDR),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
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

#[test]
fn from_pairs_without_metrics_bind_addr_reports_missing_key() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MissingKey {
            key: KEY_METRICS_BIND_ADDR,
        })
    );
}

#[test]
fn from_pairs_with_unparsable_metrics_bind_addr_reports_invalid_metrics_bind_addr() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_METRICS_BIND_ADDR, "not-an-address"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::InvalidMetricsBindAddr {
            value: "not-an-address".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unspecified_metrics_bind_addr_reports_metrics_requires_tls() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_METRICS_BIND_ADDR, "0.0.0.0:9100"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MetricsRequiresTls {
            value: "0.0.0.0:9100".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_non_loopback_metrics_bind_addr_reports_metrics_requires_tls() {
    let result = NodeConfig::from_pairs(&[
        (KEY_NODE_ID, NODE_ID),
        (KEY_LISTEN_ADDR, LISTEN_ADDR),
        (KEY_DATA_DIR, DATA_DIR),
        (KEY_ENCRYPTION_KEY_PATH, ENCRYPTION_KEY_PATH),
        (KEY_SIGNING_KEY_PATH, SIGNING_KEY_PATH),
        (KEY_MAX_FRAME_LEN, MAX_FRAME_LEN),
        (KEY_QUEUE_CAPACITY, QUEUE_CAPACITY),
        (KEY_LOG_FILE_PATH, LOG_FILE_PATH),
        (KEY_LOG_QUEUE_CAPACITY, LOG_QUEUE_CAPACITY),
        (KEY_LOG_MAX_FILE_BYTES, LOG_MAX_FILE_BYTES),
        (KEY_LOG_MAX_TOTAL_BYTES, LOG_MAX_TOTAL_BYTES),
        (KEY_LOG_MAX_FILE_AGE_SECONDS, LOG_MAX_FILE_AGE_SECONDS),
        (KEY_CATALOG_PAGE_SIZE, CATALOG_PAGE_SIZE),
        (KEY_CATALOG_SNAPSHOT_TTL_MS, CATALOG_SNAPSHOT_TTL_MS),
        (KEY_MAX_SOURCE_NAME_LEN, MAX_SOURCE_NAME_LEN),
        (KEY_METRICS_BIND_ADDR, "192.0.2.1:9100"),
    ]);

    assert_eq!(
        result,
        Err(ConfigError::MetricsRequiresTls {
            value: "192.0.2.1:9100".to_owned(),
        })
    );
}

#[test]
fn from_pairs_without_log_file_path_reports_missing_key() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_without(KEY_LOG_FILE_PATH)),
        Err(ConfigError::MissingKey {
            key: KEY_LOG_FILE_PATH,
        })
    );
}

#[test]
fn from_pairs_without_log_queue_capacity_reports_missing_key() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_without(KEY_LOG_QUEUE_CAPACITY)),
        Err(ConfigError::MissingKey {
            key: KEY_LOG_QUEUE_CAPACITY,
        })
    );
}

#[test]
fn from_pairs_without_log_max_file_bytes_reports_missing_key() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_without(KEY_LOG_MAX_FILE_BYTES)),
        Err(ConfigError::MissingKey {
            key: KEY_LOG_MAX_FILE_BYTES,
        })
    );
}

#[test]
fn from_pairs_without_log_max_total_bytes_reports_missing_key() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_without(KEY_LOG_MAX_TOTAL_BYTES)),
        Err(ConfigError::MissingKey {
            key: KEY_LOG_MAX_TOTAL_BYTES,
        })
    );
}

#[test]
fn from_pairs_without_log_max_file_age_seconds_reports_missing_key() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_without(KEY_LOG_MAX_FILE_AGE_SECONDS)),
        Err(ConfigError::MissingKey {
            key: KEY_LOG_MAX_FILE_AGE_SECONDS,
        })
    );
}

#[test]
fn from_pairs_with_zero_log_queue_capacity_reports_invalid_log_setting() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_with(KEY_LOG_QUEUE_CAPACITY, "0")),
        Err(ConfigError::InvalidLogSetting {
            key: KEY_LOG_QUEUE_CAPACITY,
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_zero_log_max_file_bytes_reports_invalid_log_setting() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_with(KEY_LOG_MAX_FILE_BYTES, "0")),
        Err(ConfigError::InvalidLogSetting {
            key: KEY_LOG_MAX_FILE_BYTES,
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_zero_log_max_total_bytes_reports_invalid_log_setting() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_with(KEY_LOG_MAX_TOTAL_BYTES, "0")),
        Err(ConfigError::InvalidLogSetting {
            key: KEY_LOG_MAX_TOTAL_BYTES,
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_zero_log_max_file_age_seconds_reports_invalid_log_setting() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_with(KEY_LOG_MAX_FILE_AGE_SECONDS, "0")),
        Err(ConfigError::InvalidLogSetting {
            key: KEY_LOG_MAX_FILE_AGE_SECONDS,
            value: "0".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_unparsable_log_max_file_bytes_reports_invalid_log_setting() {
    assert_eq!(
        NodeConfig::from_pairs(&pairs_with(KEY_LOG_MAX_FILE_BYTES, "not-a-number")),
        Err(ConfigError::InvalidLogSetting {
            key: KEY_LOG_MAX_FILE_BYTES,
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn from_pairs_with_total_log_size_below_one_file_reports_log_config_error() {
    let pairs = pairs_with(KEY_LOG_MAX_TOTAL_BYTES, "1024");

    assert_eq!(
        NodeConfig::from_pairs(&pairs),
        Err(ConfigError::Log(LogConfigError::TotalSmallerThanFile))
    );
}
