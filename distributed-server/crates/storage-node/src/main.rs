//! `storage-node` binary entry point.
//!
//! Reads and validates the node configuration from environment variables,
//! loads the configured private key files, prepares the accounts and raft
//! directories under the configured data root, emits a structured startup log,
//! binds the listen address, and serves frame connections until it receives a
//! Ctrl-C shutdown signal. The `io_uring` disk thread is a later increment.

use std::process::ExitCode;
use std::sync::Arc;

use storage_node::{
    serve, start_logging, Listener, LogEvent, LogLevel, Logger, Metrics, MetricsServer, NodeConfig,
    PrivateKey, StateChange, StorageLayout, PROTOCOL_VERSION,
};

/// Default log filter used when `RUST_LOG` is unset or unparsable.
const DEFAULT_LOG_FILTER: &str = "info";

/// Environment variable carrying the node identity label.
const ENV_NODE_ID: &str = "NODE_ID";
/// Environment variable carrying the listen address.
const ENV_LISTEN_ADDR: &str = "LISTEN_ADDR";
/// Environment variable carrying the data directory.
const ENV_DATA_DIR: &str = "DATA_DIR";
/// Environment variable carrying the encryption key file path.
const ENV_ENCRYPTION_KEY_PATH: &str = "ENCRYPTION_KEY_PATH";
/// Environment variable carrying the signing key file path.
const ENV_SIGNING_KEY_PATH: &str = "SIGNING_KEY_PATH";
/// Environment variable carrying the maximum frame length.
const ENV_MAX_FRAME_LEN: &str = "MAX_FRAME_LEN";
/// Environment variable carrying the per-connection queue capacity.
const ENV_QUEUE_CAPACITY: &str = "QUEUE_CAPACITY";
/// Environment variable carrying the metrics bind address.
const ENV_METRICS_BIND_ADDR: &str = "METRICS_BIND_ADDR";
/// Environment variable carrying the catalog page size.
const ENV_CATALOG_PAGE_SIZE: &str = "CATALOG_PAGE_SIZE";
/// Environment variable carrying the catalog snapshot lifetime in milliseconds.
const ENV_CATALOG_SNAPSHOT_TTL_MS: &str = "CATALOG_SNAPSHOT_TTL_MS";
/// Environment variable carrying the maximum source-name length in bytes.
const ENV_MAX_SOURCE_NAME_LEN: &str = "MAX_SOURCE_NAME_LEN";
/// Environment variable carrying the comma-separated peer list.
const ENV_PEERS: &str = "PEERS";
/// Environment variable carrying the structured log file path.
const ENV_LOG_FILE_PATH: &str = "LOG_FILE_PATH";
/// Environment variable carrying the structured log queue capacity.
const ENV_LOG_QUEUE_CAPACITY: &str = "LOG_QUEUE_CAPACITY";
/// Environment variable carrying the maximum structured log file size.
const ENV_LOG_MAX_FILE_BYTES: &str = "LOG_MAX_FILE_BYTES";
/// Environment variable carrying the maximum total structured log size.
const ENV_LOG_MAX_TOTAL_BYTES: &str = "LOG_MAX_TOTAL_BYTES";
/// Environment variable carrying the structured log file age in seconds.
const ENV_LOG_MAX_FILE_AGE_SECONDS: &str = "LOG_MAX_FILE_AGE_SECONDS";

/// `from_pairs` key for the node identity label.
const KEY_NODE_ID: &str = "node_id";
/// `from_pairs` key for the listen address.
const KEY_LISTEN_ADDR: &str = "listen_addr";
/// `from_pairs` key for the data directory.
const KEY_DATA_DIR: &str = "data_dir";
/// `from_pairs` key for the encryption key file path.
const KEY_ENCRYPTION_KEY_PATH: &str = "encryption_key_path";
/// `from_pairs` key for the signing key file path.
const KEY_SIGNING_KEY_PATH: &str = "signing_key_path";
/// `from_pairs` key for the maximum frame length.
const KEY_MAX_FRAME_LEN: &str = "max_frame_len";
/// `from_pairs` key for the per-connection queue capacity.
const KEY_QUEUE_CAPACITY: &str = "queue_capacity";
/// `from_pairs` key for the metrics bind address.
const KEY_METRICS_BIND_ADDR: &str = "metrics_bind_addr";
/// `from_pairs` key for the catalog page size.
const KEY_CATALOG_PAGE_SIZE: &str = "catalog_page_size";
/// `from_pairs` key for the catalog snapshot lifetime in milliseconds.
const KEY_CATALOG_SNAPSHOT_TTL_MS: &str = "catalog_snapshot_ttl_ms";
/// `from_pairs` key for the maximum source-name length in bytes.
const KEY_MAX_SOURCE_NAME_LEN: &str = "max_source_name_len";
/// `from_pairs` key for the peer list.
const KEY_PEERS: &str = "peers";
/// `from_pairs` key for the structured log file path.
const KEY_LOG_FILE_PATH: &str = "log_file_path";
/// `from_pairs` key for the structured log queue capacity.
const KEY_LOG_QUEUE_CAPACITY: &str = "log_queue_capacity";
/// `from_pairs` key for the maximum structured log file size.
const KEY_LOG_MAX_FILE_BYTES: &str = "log_max_file_bytes";
/// `from_pairs` key for the maximum total structured log size.
const KEY_LOG_MAX_TOTAL_BYTES: &str = "log_max_total_bytes";
/// `from_pairs` key for the structured log file age in seconds.
const KEY_LOG_MAX_FILE_AGE_SECONDS: &str = "log_max_file_age_seconds";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    init_tracing();
    run().await
}

/// Installs the global subscriber, honouring `RUST_LOG`.
///
/// A second initialization attempt is ignored so repeated setup cannot panic.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Loads and validates configuration and its keys, prepares the data layout,
/// then serves until stopped.
///
/// A missing environment variable is treated as an empty value so validation
/// reports a missing key instead of aborting.
async fn run() -> ExitCode {
    let node_id = env_value(ENV_NODE_ID);
    let listen_addr = env_value(ENV_LISTEN_ADDR);
    let data_dir = env_value(ENV_DATA_DIR);
    let encryption_key_path = env_value(ENV_ENCRYPTION_KEY_PATH);
    let signing_key_path = env_value(ENV_SIGNING_KEY_PATH);
    let max_frame_len = env_value(ENV_MAX_FRAME_LEN);
    let queue_capacity = env_value(ENV_QUEUE_CAPACITY);
    let metrics_bind_addr = env_value(ENV_METRICS_BIND_ADDR);
    let catalog_page_size = env_value(ENV_CATALOG_PAGE_SIZE);
    let catalog_snapshot_ttl_ms = env_value(ENV_CATALOG_SNAPSHOT_TTL_MS);
    let max_source_name_len = env_value(ENV_MAX_SOURCE_NAME_LEN);
    let peers = env_value(ENV_PEERS);
    let log_file_path = env_value(ENV_LOG_FILE_PATH);
    let log_queue_capacity = env_value(ENV_LOG_QUEUE_CAPACITY);
    let log_max_file_bytes = env_value(ENV_LOG_MAX_FILE_BYTES);
    let log_max_total_bytes = env_value(ENV_LOG_MAX_TOTAL_BYTES);
    let log_max_file_age_seconds = env_value(ENV_LOG_MAX_FILE_AGE_SECONDS);

    let pairs = [
        (KEY_NODE_ID, node_id.as_str()),
        (KEY_LISTEN_ADDR, listen_addr.as_str()),
        (KEY_DATA_DIR, data_dir.as_str()),
        (KEY_ENCRYPTION_KEY_PATH, encryption_key_path.as_str()),
        (KEY_SIGNING_KEY_PATH, signing_key_path.as_str()),
        (KEY_MAX_FRAME_LEN, max_frame_len.as_str()),
        (KEY_QUEUE_CAPACITY, queue_capacity.as_str()),
        (KEY_METRICS_BIND_ADDR, metrics_bind_addr.as_str()),
        (KEY_LOG_FILE_PATH, log_file_path.as_str()),
        (KEY_LOG_QUEUE_CAPACITY, log_queue_capacity.as_str()),
        (KEY_LOG_MAX_FILE_BYTES, log_max_file_bytes.as_str()),
        (KEY_LOG_MAX_TOTAL_BYTES, log_max_total_bytes.as_str()),
        (
            KEY_LOG_MAX_FILE_AGE_SECONDS,
            log_max_file_age_seconds.as_str(),
        ),
        (KEY_CATALOG_PAGE_SIZE, catalog_page_size.as_str()),
        (
            KEY_CATALOG_SNAPSHOT_TTL_MS,
            catalog_snapshot_ttl_ms.as_str(),
        ),
        (KEY_MAX_SOURCE_NAME_LEN, max_source_name_len.as_str()),
        (KEY_PEERS, peers.as_str()),
    ];

    let config = match NodeConfig::from_pairs(&pairs) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };
    let logging = match start_logging(config.log.clone()) {
        Ok(logging) => logging,
        Err(error) => {
            tracing::error!(error = %error, "failed to start structured logging");
            return ExitCode::FAILURE;
        }
    };
    let logger = logging.logger().clone();
    logger.log(LogEvent::state_change(LogLevel::Info, StateChange::Started));

    let success = run_node(&config, &logger).await;
    let stopped_level = if success {
        LogLevel::Info
    } else {
        LogLevel::Error
    };
    logger.log(LogEvent::state_change(stopped_level, StateChange::Stopped));
    drop(logger);
    logging.shutdown().await;

    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Loads keys, prepares the layout, binds the listeners, and serves until stopped.
async fn run_node(config: &NodeConfig, logger: &Logger) -> bool {
    let _encryption_key = match PrivateKey::load(&config.encryption_key_path) {
        Ok(key) => key,
        Err(error) => {
            tracing::error!(error = %error, "failed to load the encryption key");
            logger.log(LogEvent::state_change(
                LogLevel::Error,
                StateChange::DependencyUnavailable,
            ));
            return false;
        }
    };
    let _signing_key = match PrivateKey::load(&config.signing_key_path) {
        Ok(key) => key,
        Err(error) => {
            tracing::error!(error = %error, "failed to load the signing key");
            logger.log(LogEvent::state_change(
                LogLevel::Error,
                StateChange::DependencyUnavailable,
            ));
            return false;
        }
    };

    tracing::info!(
        crate_version = env!("CARGO_PKG_VERSION"),
        protocol_version = PROTOCOL_VERSION,
        node_id = %config.node_id,
        listen_addr = %config.listen_addr,
        data_dir = %config.data_dir.display(),
        catalog_page_size = config.catalog_page_size.get(),
        catalog_snapshot_ttl_ms = config.catalog_snapshot_ttl_ms.get(),
        max_source_name_len = config.max_source_name_len.get(),
        peers = config.peers.len(),
        "storage node starting"
    );

    let layout = StorageLayout::new(config.data_dir.clone());
    if let Err(error) = layout.ensure_dirs() {
        tracing::error!(error = %error, "failed to prepare the data layout");
        logger.log(LogEvent::state_change(
            LogLevel::Error,
            StateChange::DependencyUnavailable,
        ));
        return false;
    }
    tracing::debug!(root = %layout.root.display(), "data layout ready");
    logger.log(LogEvent::state_change(
        LogLevel::Info,
        StateChange::LayoutReady,
    ));

    let listener = match Listener::bind(&config.listen_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(error = %error, "failed to bind listener");
            logger.log(LogEvent::state_change(
                LogLevel::Error,
                StateChange::DependencyUnavailable,
            ));
            return false;
        }
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            tracing::error!(error = %error, "listener address unavailable");
            logger.log(LogEvent::state_change(
                LogLevel::Error,
                StateChange::DependencyUnavailable,
            ));
            return false;
        }
    };
    tracing::info!(%address, "listening");
    logger.log(LogEvent::state_change(
        LogLevel::Info,
        StateChange::ListenerBound,
    ));

    let metrics = Arc::new(Metrics::new(config.queue_capacity));
    let metrics_server =
        match MetricsServer::bind(config.metrics_bind_addr, Arc::clone(&metrics)).await {
            Ok(server) => server,
            Err(error) => {
                tracing::error!(error = %error, "failed to bind metrics listener");
                logger.log(LogEvent::state_change(
                    LogLevel::Error,
                    StateChange::DependencyUnavailable,
                ));
                return false;
            }
        };
    let metrics_addr = metrics_server.local_addr().ok();
    tracing::info!(metrics_addr = ?metrics_addr, "metrics listening");
    logger.log(LogEvent::state_change(
        LogLevel::Info,
        StateChange::MetricsBound,
    ));
    let metrics_task = tokio::spawn(metrics_server.serve());

    let outcome = serve(
        listener,
        shutdown_signal(),
        config.max_frame_len,
        config.queue_capacity,
        Some(logger.clone()),
    )
    .await;
    metrics_task.abort();

    match outcome {
        Ok(()) => {
            tracing::info!("storage node stopped");
            true
        }
        Err(error) => {
            tracing::error!(error = %error, "node runtime failed");
            false
        }
    }
}

/// Resolves when the process receives Ctrl-C, failing open on a listen error.
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %error, "failed to listen for the shutdown signal");
    }
}

/// Reads an environment variable, mapping an absent variable to an empty string.
fn env_value(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
