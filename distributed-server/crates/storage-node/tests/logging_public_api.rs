//! Public structured logging boundary.
//!
//! Covers `NFR-OBS-001 / 1`: UTF-8 JSON Lines with linked request lifecycle
//! events plus duration and error fields. Covers `NFR-OBS-002 / 1`: state
//! changes and error records. Covers `NFR-OBS-003 / 1`: records carry no user
//! data or secrets. Covers `NFR-OBS-004 / 1`: a bounded queue feeds the writer.
//! Covers `CFG-OBS-001 / 1`: the log file follows the configured path. Covers
//! `CFG-OBS-002 / 1`: file-size, file-age, and total-size retention limits.

use std::fs;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use storage_node::{
    start_logging, ErrorCategory, LogConfig, LogEvent, LogLevel, OperationType, Outcome,
    StateChange,
};
use tempfile::TempDir;

/// Component name every structured record must carry.
const COMPONENT: &str = "storage-node";
/// Name of the current log file inside the temporary directory.
const EVENT_FILE: &str = "events.jsonl";
/// Prefix shared by every rotated log file.
const ROTATED_PREFIX: &str = "events.jsonl.";
/// Request id shared by the accepted and finished events.
const REQUEST_ID: &str = "req-1";
/// Upload id attached to the finished event.
const UPLOAD_ID: &str = "00112233445566778899aabbccddeeff";
/// Peer that accepted the request.
const PEER_ONE: &str = "peer-1";
/// Peer whose connection was rejected.
const PEER_TWO: &str = "peer-2";
/// Duration recorded for the finished request.
const EVENT_DURATION_MS: u64 = 42;
/// Ages in this test file are configured in whole seconds.
const MAX_FILE_AGE_SECONDS: u64 = 60;

/// Keys that would leak credentials, user metadata, or payload bytes.
const FORBIDDEN_KEYS: [&str; 7] = [
    "login",
    "password",
    "file_name",
    "payload",
    "verifier",
    "key",
    "private_key",
];
/// User values that must not appear anywhere in the log output.
const FORBIDDEN_VALUES: [&str; 2] = ["alice", "photo.jpg"];

/// Builds a validated log configuration with explicit test limits.
fn log_config(
    path: PathBuf,
    queue_capacity: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_file_age_seconds: u64,
) -> LogConfig {
    LogConfig::new(
        path,
        NonZeroUsize::new(queue_capacity).expect("test queue capacity is non-zero"),
        NonZeroU64::new(max_file_bytes).expect("test file limit is non-zero"),
        NonZeroU64::new(max_total_bytes).expect("test total limit is non-zero"),
        NonZeroU64::new(max_file_age_seconds).expect("test file age is non-zero"),
    )
    .expect("test log configuration is valid")
}

/// Parses every line of a log file as one JSON object.
fn records(path: &Path) -> Vec<serde_json::Value> {
    let contents = fs::read_to_string(path).expect("the log file is readable UTF-8");
    contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("each log line is a JSON object"))
        .collect()
}

/// Returns the millisecond timestamp encoded in a rotated file name.
fn rotated_timestamp_ms(name: &str) -> Option<u128> {
    name.strip_prefix(ROTATED_PREFIX)?
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Returns the current UTC time in milliseconds.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the test clock is after the Unix epoch")
        .as_millis()
}

#[tokio::test]
async fn writes_json_lines_with_the_required_schema_and_no_user_data() {
    let directory = TempDir::new().expect("test temp dir is created");
    let path = directory.path().join(EVENT_FILE);
    let logging = start_logging(log_config(
        path.clone(),
        16,
        4096,
        65536,
        MAX_FILE_AGE_SECONDS,
    ))
    .expect("structured logging starts on the test runtime");
    let logger = logging.logger().clone();

    logger.log(LogEvent::state_change(LogLevel::Info, StateChange::Started));
    logger.log(LogEvent::request_accepted(
        OperationType::Upload,
        REQUEST_ID,
        Some(PEER_ONE),
    ));
    logger.log(
        LogEvent::request_finished(
            OperationType::Upload,
            REQUEST_ID,
            Outcome::Success,
            EVENT_DURATION_MS,
            None,
            None,
        )
        .with_upload_id(UPLOAD_ID),
    );
    logger.log(LogEvent::connection_rejected(
        Some(PEER_TWO),
        ErrorCategory::InvalidRequest,
    ));
    drop(logger);
    logging.shutdown().await;

    let contents = fs::read_to_string(&path).expect("the log file is readable UTF-8");
    let records: Vec<serde_json::Value> = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("each log line is a JSON object"))
        .collect();
    assert_eq!(records.len(), 4, "each emitted event becomes one line");

    for record in &records {
        assert!(record.is_object(), "every record is a JSON object");
        assert!(
            record["timestamp_ms"].is_u64(),
            "every record carries a numeric UTC timestamp"
        );
        assert_eq!(record["component"].as_str(), Some(COMPONENT));
        assert!(
            record["level"].is_string(),
            "every record carries an importance level"
        );
        assert!(
            record["event_type"].is_string(),
            "every record carries an event type"
        );
    }

    assert_eq!(records[0]["event_type"].as_str(), Some("state_change"));
    assert_eq!(records[1]["event_type"].as_str(), Some("request_accepted"));
    assert_eq!(records[2]["event_type"].as_str(), Some("request_finished"));
    assert_eq!(
        records[3]["event_type"].as_str(),
        Some("connection_rejected")
    );

    assert_eq!(records[1]["request_id"].as_str(), Some(REQUEST_ID));
    assert_eq!(records[1]["peer_id"].as_str(), Some(PEER_ONE));
    assert_eq!(
        records[2]["request_id"].as_str(),
        records[1]["request_id"].as_str(),
        "accepted and finished events share one request id"
    );
    assert_eq!(records[2]["upload_id"].as_str(), Some(UPLOAD_ID));
    assert_eq!(records[2]["duration_ms"].as_u64(), Some(EVENT_DURATION_MS));
    assert_eq!(
        records[3]["error_category"].as_str(),
        Some("invalid_request"),
        "connection rejection carries a safe error category"
    );

    for record in &records {
        let object = record
            .as_object()
            .expect("the schema check already proved every record is an object");
        for key in FORBIDDEN_KEYS {
            assert!(
                !object.contains_key(key),
                "a log record must not contain the {key} key"
            );
        }
    }
    for value in FORBIDDEN_VALUES {
        assert!(
            !contents.contains(value),
            "the log output must not contain the {value} value"
        );
    }
}

#[tokio::test]
async fn rotates_and_prunes_to_the_configured_size() {
    const MAX_FILE_BYTES: u64 = 256;
    const MAX_TOTAL_BYTES: u64 = 512;

    let directory = TempDir::new().expect("test temp dir is created");
    let path = directory.path().join(EVENT_FILE);
    let logging = start_logging(log_config(
        path.clone(),
        64,
        MAX_FILE_BYTES,
        MAX_TOTAL_BYTES,
        MAX_FILE_AGE_SECONDS,
    ))
    .expect("structured logging starts on the test runtime");
    let logger = logging.logger().clone();

    for index in 0..100u32 {
        logger.log(LogEvent::request_finished(
            OperationType::Upload,
            &format!("req-{index:04}"),
            Outcome::Success,
            1,
            None,
            None,
        ));
    }
    drop(logger);
    logging.shutdown().await;

    let entries: Vec<_> = fs::read_dir(directory.path())
        .expect("the log directory is readable")
        .map(|entry| entry.expect("each log directory entry is readable"))
        .collect();
    let mut total_bytes = 0;
    let mut rotated = false;
    for entry in &entries {
        let metadata = entry.metadata().expect("log file metadata is readable");
        if !metadata.is_file() {
            continue;
        }
        total_bytes += metadata.len();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        rotated |= name.starts_with(ROTATED_PREFIX);
        for line in fs::read_to_string(entry.path())
            .expect("each log file is readable UTF-8")
            .lines()
        {
            serde_json::from_str::<serde_json::Value>(line).expect("every log line is valid JSON");
        }
    }

    assert!(rotated, "the size limit forces at least one rotation");
    assert!(
        total_bytes <= MAX_TOTAL_BYTES,
        "retention keeps the total log size within {MAX_TOTAL_BYTES} bytes"
    );
}

#[tokio::test]
async fn enforces_the_configured_file_age() {
    const MAX_FILE_AGE_SECONDS: u64 = 1;

    let directory = TempDir::new().expect("test temp dir is created");
    let path = directory.path().join(EVENT_FILE);
    let logging = start_logging(log_config(
        path.clone(),
        16,
        4096,
        65536,
        MAX_FILE_AGE_SECONDS,
    ))
    .expect("structured logging starts on the test runtime");
    let logger = logging.logger().clone();

    logger.log(LogEvent::request_finished(
        OperationType::Upload,
        "req-old",
        Outcome::Success,
        1,
        None,
        None,
    ));
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    logger.log(LogEvent::request_finished(
        OperationType::Upload,
        "req-new",
        Outcome::Success,
        2,
        None,
        None,
    ));
    drop(logger);
    logging.shutdown().await;

    const ROTATION_TIMESTAMP_TOLERANCE_MS: u128 = 100;

    let now = now_millis();
    let allowed_age_ms = u128::from(MAX_FILE_AGE_SECONDS) * 1_000 + ROTATION_TIMESTAMP_TOLERANCE_MS;
    let oldest_allowed = now.saturating_sub(allowed_age_ms);
    for entry in fs::read_dir(directory.path()).expect("the log directory is readable") {
        let entry = entry.expect("each log directory entry is readable");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(timestamp) = rotated_timestamp_ms(&name) else {
            continue;
        };
        assert!(
            timestamp >= oldest_allowed,
            "the rotated file {name} is within the configured age budget"
        );
    }

    let parsed = records(&path);
    assert_eq!(
        parsed.len(),
        1,
        "the aged file is rotated before the new event is written"
    );
    assert_eq!(parsed[0]["request_id"].as_str(), Some("req-new"));
}

#[tokio::test]
async fn rotates_an_aged_current_file_at_startup() {
    const MAX_FILE_AGE_SECONDS: u64 = 1;

    let directory = TempDir::new().expect("test temp dir is created");
    let path = directory.path().join(EVENT_FILE);
    let config = log_config(path.clone(), 16, 4096, 65536, MAX_FILE_AGE_SECONDS);

    let logging = start_logging(config.clone()).expect("structured logging starts");
    let logger = logging.logger().clone();
    logger.log(LogEvent::request_finished(
        OperationType::Upload,
        "req-old",
        Outcome::Success,
        1,
        None,
        None,
    ));
    drop(logger);
    logging.shutdown().await;

    tokio::time::sleep(Duration::from_millis(1200)).await;

    let logging = start_logging(config).expect("structured logging restarts");
    let logger = logging.logger().clone();
    logger.log(LogEvent::request_finished(
        OperationType::Upload,
        "req-new",
        Outcome::Success,
        2,
        None,
        None,
    ));
    drop(logger);
    logging.shutdown().await;

    let current = records(&path);
    assert_eq!(
        current.len(),
        1,
        "the aged current file is rotated before the new event"
    );
    assert_eq!(current[0]["request_id"].as_str(), Some("req-new"));

    let rotated = fs::read_dir(directory.path())
        .expect("the log directory is readable")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(ROTATED_PREFIX)
        })
        .expect("the aged current file was rotated");
    let rotated_records = records(&rotated.path());
    assert_eq!(rotated_records.len(), 1);
    assert_eq!(rotated_records[0]["request_id"].as_str(), Some("req-old"));
}

#[tokio::test]
async fn configures_the_log_file_path() {
    let directory = TempDir::new().expect("test temp dir is created");
    let path = directory.path().join("sub").join(EVENT_FILE);
    let logging = start_logging(log_config(
        path.clone(),
        16,
        4096,
        65536,
        MAX_FILE_AGE_SECONDS,
    ))
    .expect("structured logging creates the configured parent directory");

    logging
        .logger()
        .log(LogEvent::state_change(LogLevel::Info, StateChange::Started));
    logging.shutdown().await;

    assert!(path.is_file(), "logging writes to the configured path");
    let parsed = records(&path);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["component"].as_str(), Some(COMPONENT));
}
