//! Dedicated server (control plane) for the HomeCloud v1 cluster.
//!
//! This crate implements configuration loading and validation, password-verifier
//! derivation and checking, heartbeat tracking, account registration, and key
//! identity loading; networking and the wire protocol are still to come.

mod clock;
mod config;
mod heartbeat;
mod identity;
mod logging;
mod password;
mod registration;
mod report;
mod topology;

pub use clock::{ClockOffset, ClockSample, ExchangeTimestamps};
pub use config::{
    ConfigError, NodeField, NodeId, PublicKey, ServerConfig, CONFIG_PATH_ENV,
    DEFAULT_CONFIG_FILE_NAME,
};
pub use heartbeat::{HeartbeatAction, HeartbeatError, HeartbeatTiming, HeartbeatTracker};
pub use identity::{IdentityError, ServerIdentity};
pub use logging::{
    init, logging_failed_to_stderr, shutdown, startup, startup_failed, startup_failed_to_stderr,
    LogError, LogGuard,
};
pub use password::{Argon2Params, PasswordError, PasswordVerifier};
pub use registration::{
    Account, AccountRegistry, Installation, InstallationId, InstallationKeys, Login, LoginError,
    RegistrationError, RegistrationOutcome, RegistrationRequest, RequestId,
};
pub use report::{Startup, StartupError};
pub use topology::{Topology, TopologyNode};
