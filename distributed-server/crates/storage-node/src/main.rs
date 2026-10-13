//! Minimal `storage-node` binary entry point.
//!
//! The actor runtime, TCP listener, and `io_uring` disk thread are later
//! increments; this only reports the build's crate and protocol versions.

use storage_node::PROTOCOL_VERSION;

fn main() {
    println!(
        "storage-node {} (protocol version {})",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL_VERSION
    );
}
