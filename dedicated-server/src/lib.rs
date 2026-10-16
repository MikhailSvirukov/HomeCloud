//! Dedicated server (control plane) for the HomeCloud v1 cluster.
//!
//! This crate implements configuration loading and validation, heartbeat
//! tracking, account registration, and key identity loading; networking and the
//! wire protocol are still to come.

mod config;
mod heartbeat;
mod identity;
mod registration;
mod report;

pub use config::{ConfigError, ServerConfig, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME};
pub use heartbeat::{HeartbeatAction, HeartbeatError, HeartbeatTiming, HeartbeatTracker};
pub use identity::{IdentityError, ServerIdentity};
pub use registration::{Account, AccountRegistry, Login, LoginError, RegistrationError};
pub use report::{Startup, StartupError};
