//! Startup wiring of a validated configuration and its key identity.
//!
//! Loading a [`Startup`] from a path reads the configuration and then delegates
//! to [`Startup::from_config`], which loads the key identity named by that
//! configuration, so the binary never touches configuration fields directly.
//! The two-phase entry point loads the configuration first because it names the
//! structured log file. The summary renders the loaded values for the startup
//! log line.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{ConfigError, ServerConfig};
use crate::identity::{IdentityError, ServerIdentity};
use crate::inter_server_client::InterServerClientError;
use crate::registration::{
    AccountRegistry, RegistrationError, RegistrationOutcome, RegistrationRequest,
};
use crate::registration_tag::{RegistrationSecret, RegistrationSecretError};
use crate::topology::Topology;

/// Result of loading a [`Startup`].
type Result<T> = core::result::Result<T, StartupError>;

/// A dedicated server that loaded its configuration and its key identity.
pub struct Startup {
    config: ServerConfig,
    identity: ServerIdentity,
    accounts: AccountRegistry,
    registration_secret: RegistrationSecret,
}

impl Startup {
    /// Load the configuration at `path` and then its key identity.
    pub fn load(path: &Path) -> Result<Self> {
        let config = ServerConfig::load(path).map_err(|source| StartupError::Configuration {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_config(config)
    }

    /// Load the key identity named by an already loaded configuration.
    pub fn from_config(config: ServerConfig) -> Result<Self> {
        let identity = ServerIdentity::load(
            &config.dedicated.label,
            &config.identity.encryption_private_key,
            &config.identity.signing_private_key,
        )
        .map_err(|source| StartupError::Identity { source })?;

        let accounts = AccountRegistry::new(config.password.params(), config.credential_limits());
        let registration_secret = RegistrationSecret::load(&config.registration.secret_path)
            .map_err(|source| StartupError::RegistrationSecret { source })?;

        Ok(Self {
            config,
            identity,
            accounts,
            registration_secret,
        })
    }

    /// Propose one registration command to the first storage node in the manifest.
    pub fn propose_registration(
        &self,
        request: crate::inter_server_client::RegistrationProposeRequest<'_>,
    ) -> core::result::Result<crate::inter_server_client::RegistrationCommit, InterServerClientError>
    {
        crate::inter_server_client::propose_registration(
            &self.config,
            &self.identity,
            &self.registration_secret,
            &request,
        )
    }

    /// Look up the registry of the first storage node in the static manifest.
    pub fn lookup_registry(
        &self,
        login: Option<&str>,
        registration_request_id: Option<[u8; crate::wire::IDENTIFIER_BYTES]>,
        installation_id: Option<[u8; crate::wire::IDENTIFIER_BYTES]>,
    ) -> core::result::Result<crate::wire::RegistrySnapshot, InterServerClientError> {
        crate::inter_server_client::lookup_registry(
            &self.config,
            &self.identity,
            login,
            registration_request_id,
            installation_id,
        )
    }

    /// The registration secret `K` loaded at startup.
    pub fn registration_secret(&self) -> &RegistrationSecret {
        &self.registration_secret
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
        Topology::from_manifest(&self.config.storage)
    }

    /// Configured cluster labels, in manifest order.
    pub fn cluster_labels(&self) -> Vec<&str> {
        self.config
            .storage
            .iter()
            .map(|node| node.label.as_str())
            .collect()
    }

    /// Prometheus metrics endpoint configuration.
    pub fn metrics(&self) -> &crate::config::MetricsConfig {
        &self.config.metrics
    }

    /// One-line summary of the configuration and identity the server started with.
    pub fn summary(&self) -> String {
        format!(
            "dedicated-server listening on {} label={} nodes={} interval_ms={} timeout_ms={} miss_threshold={} encryption_key_bytes={} signing_key_bytes={}",
            self.config.listen,
            self.identity.label(),
            self.config.storage.len(),
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
    #[error("failed to load key identity: {source}")]
    Identity {
        /// Underlying identity failure.
        #[source]
        source: IdentityError,
    },
    /// Loading the registration secret named by the configuration failed.
    #[error("failed to load the registration secret: {source}")]
    RegistrationSecret {
        /// Underlying secret failure.
        #[source]
        source: RegistrationSecretError,
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
