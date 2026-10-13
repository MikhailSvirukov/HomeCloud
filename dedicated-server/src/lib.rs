//! Dedicated server (control plane) for the HomeCloud v1 cluster.
//!
//! This crate currently provides configuration loading and validation only;
//! networking, registration and heartbeat runtime behavior are added later.

pub mod config;
