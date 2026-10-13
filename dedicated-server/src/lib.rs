//! Dedicated server (control plane) for the HomeCloud v1 cluster.
//!
//! This crate implements configuration loading and validation, heartbeat
//! tracking, account registration, and key identity loading; networking and the
//! wire protocol are still to come.

pub mod config;
pub mod heartbeat;
pub mod identity;
pub mod registration;
