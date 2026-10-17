//! `storage-node` binary entry point.
//!
//! Reads and validates the node configuration from environment variables,
//! prepares the accounts and raft directories under the configured data root,
//! emits a structured startup log, binds the listen address, and serves frame
//! connections until it receives a Ctrl-C shutdown signal. The `io_uring` disk
//! thread is a later increment.

use std::process::ExitCode;

use storage_node::{serve, Listener, NodeConfig, StorageLayout, PROTOCOL_VERSION};

/// Default log filter used when `RUST_LOG` is unset or unparsable.
const DEFAULT_LOG_FILTER: &str = "info";

/// Environment variable carrying the node identity label.
const ENV_NODE_ID: &str = "NODE_ID";
/// Environment variable carrying the listen address.
const ENV_LISTEN_ADDR: &str = "LISTEN_ADDR";
/// Environment variable carrying the data directory.
const ENV_DATA_DIR: &str = "DATA_DIR";
/// Environment variable carrying the comma-separated peer list.
const ENV_PEERS: &str = "PEERS";

/// `from_pairs` key for the node identity label.
const KEY_NODE_ID: &str = "node_id";
/// `from_pairs` key for the listen address.
const KEY_LISTEN_ADDR: &str = "listen_addr";
/// `from_pairs` key for the data directory.
const KEY_DATA_DIR: &str = "data_dir";
/// `from_pairs` key for the peer list.
const KEY_PEERS: &str = "peers";

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

/// Loads and validates configuration, prepares the data layout, then serves until stopped.
///
/// A missing environment variable is treated as an empty value so validation
/// reports a missing key instead of aborting.
async fn run() -> ExitCode {
    let node_id = env_value(ENV_NODE_ID);
    let listen_addr = env_value(ENV_LISTEN_ADDR);
    let data_dir = env_value(ENV_DATA_DIR);
    let peers = env_value(ENV_PEERS);

    let pairs = [
        (KEY_NODE_ID, node_id.as_str()),
        (KEY_LISTEN_ADDR, listen_addr.as_str()),
        (KEY_DATA_DIR, data_dir.as_str()),
        (KEY_PEERS, peers.as_str()),
    ];

    let config = match NodeConfig::from_pairs(&pairs) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(
        crate_version = env!("CARGO_PKG_VERSION"),
        protocol_version = PROTOCOL_VERSION,
        node_id = %config.node_id,
        listen_addr = %config.listen_addr,
        data_dir = %config.data_dir.display(),
        peers = config.peers.len(),
        "storage node starting"
    );

    let layout = StorageLayout::new(config.data_dir.clone());
    if let Err(error) = layout.ensure_dirs() {
        tracing::error!(error = %error, "failed to prepare the data layout");
        return ExitCode::FAILURE;
    }
    tracing::debug!(root = %layout.root.display(), "data layout ready");

    let listener = match Listener::bind(&config.listen_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(error = %error, "failed to bind listener");
            return ExitCode::FAILURE;
        }
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            tracing::error!(error = %error, "listener address unavailable");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(%address, "listening");

    match serve(listener, shutdown_signal()).await {
        Ok(()) => {
            tracing::info!("storage node stopped");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(error = %error, "node runtime failed");
            ExitCode::FAILURE
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
