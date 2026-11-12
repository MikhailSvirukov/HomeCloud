//! Test-environment conformance oracle for the fixed v1 wire structures.
//!
//! This crate implements the byte-level structures published in
//! `contracts/wire-v1.md`: deterministic CBOR for the fixed headers, the frame
//! body layout, the external frame header, the result envelope, and the
//! inter-server message envelope. It is meant to produce and validate golden
//! vectors for other v1 implementations; it performs no network,
//! cryptographic, or configuration work.
//!
//! Encoding and decoding are strict: optional header positions are always
//! present as CBOR `null`, integers use the minimal deterministic form, and any
//! value outside the fixed v1 schema is rejected.

pub mod catalog;
pub mod cbor;
pub mod client;
pub mod frame;
pub mod header;
pub mod heartbeat;
pub mod host;
pub mod inter_server;
pub mod raft;
pub mod result;
pub mod topology;

pub use catalog::{CatalogEntry, CatalogError, CatalogPage, CatalogPageRequest};
pub use cbor::{CborError, Value};
pub use client::{ClientError, ClientRequest, CredentialLimits, OperationType};
pub use frame::{Frame, FrameError};
pub use header::{ExternalFrameHeader, HeaderError};
pub use heartbeat::{HeartbeatRequest, HeartbeatResponse};
pub use host::{CanonicalHost, HostError};
pub use inter_server::{
    InterServerMessage, MessageError, MessageType, RegistryError, V1_REGISTRY, validate_registry,
};
pub use raft::RaftRpc;
pub use result::{ResultClass, ResultEnvelope, ResultError};
pub use topology::{NodeEntry, TopologyAnswer, TopologyError};
