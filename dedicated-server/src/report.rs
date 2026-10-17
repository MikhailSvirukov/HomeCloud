//! Startup wiring of a validated configuration and its key identity.
//!
//! Loading a [`Startup`] reads the configuration, then loads the key identity
//! named by that configuration, so the binary never touches configuration
//! fields directly. The summary renders both for the startup log line.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{ConfigError, ServerConfig};
use crate::identity::{IdentityError, ServerIdentity};
use crate::registration::{
    AccountRegistry, RegistrationError, RegistrationOutcome, RegistrationRequest,
};
use crate::topology::Topology;

/// Result of loading a [`Startup`].
type Result<T> = core::result::Result<T, StartupError>;

/// A dedicated server that loaded its configuration and its key identity.
pub struct Startup {
    config: ServerConfig,
    identity: ServerIdentity,
    accounts: AccountRegistry,
}

impl Startup {
    /// Load the configuration at `path` together with the key identity it names.
    pub fn load(path: &Path) -> Result<Self> {
        let config = ServerConfig::load(path).map_err(|source| StartupError::Configuration {
            path: path.to_path_buf(),
            source,
        })?;

        let identity = ServerIdentity::load(
            &config.identity.label,
            &config.identity.encryption_private_key,
            &config.identity.signing_private_key,
        )
        .map_err(|source| StartupError::Identity {
            path: path.to_path_buf(),
            source,
        })?;

        let accounts = AccountRegistry::new(config.password.params());

        Ok(Self {
            config,
            identity,
            accounts,
        })
    }

    /// Register a login or an additional installation with the configured parameters.
    pub fn register(
        &mut self,
        request: RegistrationRequest,
    ) -> core::result::Result<RegistrationOutcome, RegistrationError> {
        self.accounts.register(request)
    }

    /// The topology to hand to a new client connection.
    pub fn topology(&self) -> Topology {
        Topology::from_manifest(&self.config.cluster)
    }

    /// One-line summary of the configuration and identity the server started with.
    pub fn summary(&self) -> String {
        format!(
            "dedicated-server listening on {} label={} nodes={} interval_ms={} timeout_ms={} miss_threshold={} encryption_key_bytes={} signing_key_bytes={}",
            self.config.listen,
            self.identity.label(),
            self.config.cluster.nodes.len(),
            self.config.heartbeat.interval().as_millis(),
            self.config.heartbeat.timeout().as_millis(),
            self.config.heartbeat.miss_threshold,
            self.identity.encryption_private_key().len(),
            self.identity.signing_private_key().len(),
        )
    }
}

/// Errors produced while starting the server.
#[derive(Debug, Error)]
pub enum StartupError {
    /// Reading and validating the configuration failed.
    #[error("failed to load configuration '{path}': {source}")]
    Configuration {
        /// Configuration path that was loaded.
        path: PathBuf,
        /// Underlying configuration failure.
        #[source]
        source: ConfigError,
    },
    /// Loading the key identity named by the configuration failed.
    #[error("failed to load key identity '{path}': {source}")]
    Identity {
        /// Configuration path that named the key identity.
        path: PathBuf,
        /// Underlying identity failure.
        #[source]
        source: IdentityError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const MISSING_CONFIG: &str = "/nonexistent/homecloud/dedicated-server.toml";

    #[test]
    fn load_reports_a_configuration_error_with_the_requested_path() {
        let path = Path::new(MISSING_CONFIG);

        let error = Startup::load(path)
            .err()
            .expect("a missing configuration must fail");

        assert!(matches!(
            error,
            StartupError::Configuration { path: error_path, .. } if error_path == path
        ));
    }
}
