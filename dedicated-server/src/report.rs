//! Startup summary of a validated configuration and identity.

use crate::config::ServerConfig;
use crate::identity::ServerIdentity;

/// One-line summary of the configuration and identity a server starts with.
pub fn startup_summary(config: &ServerConfig, identity: &ServerIdentity) -> String {
    format!(
        "dedicated-server listening on {} label={} nodes={} interval_ms={} timeout_ms={} miss_threshold={} encryption_key_bytes={} signing_key_bytes={}",
        config.listen,
        identity.label(),
        config.cluster.nodes.len(),
        config.heartbeat.interval().as_millis(),
        config.heartbeat.timeout().as_millis(),
        config.heartbeat.miss_threshold,
        identity.encryption_private_key().len(),
        identity.signing_private_key().len(),
    )
}
