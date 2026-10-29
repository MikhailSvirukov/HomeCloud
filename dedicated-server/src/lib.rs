//! Dedicated server (control plane) for the HomeCloud v1 cluster.
//!
//! This crate implements configuration loading and validation, password-verifier
//! derivation and checking, heartbeat tracking, account registration, and key
//! identity loading, metrics, and framing; application networking and the wire
//! protocol are still to come.

mod availability;
mod clock;
mod config;
mod frame;
mod heartbeat;
mod identity;
mod log_file;
mod logging;
mod metrics;
mod metrics_server;
mod password;
mod registration;
mod report;
mod topology;

pub use availability::{log_events, AvailabilityEvent, AvailabilityMonitor};
pub use clock::{ClockOffset, ClockSample, ExchangeTimestamps};
pub use config::{
    ConfigError, LoggingConfig, MetricsConfig, MetricsTlsConfig, NodeField, NodeId, PublicKey,
    ServerConfig, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME,
};
pub use frame::{read_frame, write_frame, FrameError, FRAME_PREFIX_BYTES};
pub use heartbeat::{HeartbeatAction, HeartbeatError, HeartbeatTiming, HeartbeatTracker};
pub use identity::{IdentityError, ServerIdentity};
pub use logging::{
    init, logging_failed_to_stderr, metrics_failed, runtime_failed, shutdown, startup,
    startup_failed, startup_failed_to_stderr, LogError, LogGuard,
};
pub use metrics::{Metrics, Operation, CONTENT_TYPE};
pub use metrics_server::{start as start_metrics_server, MetricsServer, TlsPaths, METRICS_PATH};
pub use password::{Argon2Params, PasswordError, PasswordVerifier};
pub use registration::{
    Account, AccountRegistry, Installation, InstallationId, InstallationKeys, Login, LoginError,
    RegistrationError, RegistrationOutcome, RegistrationRequest, RequestId,
};
pub use report::{Startup, StartupError};
pub use topology::{Topology, TopologyNode};
