//! `storage-node` binary entry point.
//!
//! Reads and validates the node configuration from environment variables and
//! reports a concise startup summary. The actor runtime, TCP listener, and
//! `io_uring` disk thread are later increments.

use std::process::ExitCode;

use storage_node::{NodeConfig, PROTOCOL_VERSION};

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

fn main() -> ExitCode {
    run()
}

/// Loads and validates configuration, reporting the outcome.
///
/// A missing environment variable is treated as an empty value so validation
/// reports a missing key instead of aborting.
fn run() -> ExitCode {
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

    match NodeConfig::from_pairs(&pairs) {
        Ok(config) => {
            print_startup_summary(&config);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("storage-node: invalid configuration: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Reads an environment variable, mapping an absent variable to an empty string.
fn env_value(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Prints the validated identity and endpoint summary.
fn print_startup_summary(config: &NodeConfig) {
    println!(
        "storage-node {} (protocol version {})",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL_VERSION
    );
    println!("node id: {}", config.node_id());
    println!("listen address: {}", config.listen_addr());
    println!("data dir: {}", config.data_dir().display());
    println!("peers: {}", config.peers().len());
}
