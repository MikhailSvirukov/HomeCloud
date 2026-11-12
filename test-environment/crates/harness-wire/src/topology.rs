//! Topology answer arguments and per-node rows.
//!
//! A successful topology answer carries the result arguments `[nodes]`, where
//! each row is `[sender_id, label, host, port, x25519_public_key,
//! ed25519_public_key]` in manifest order (`INT-NET-070 / 6`).

use crate::cbor::Value;
use crate::host::{CanonicalHost, HostError};

const SENDER_ID: &str = "sender_id";
const LABEL: &str = "label";
const HOST: &str = "host";
const PORT: &str = "port";
const X25519_PUBLIC_KEY: &str = "x25519_public_key";
const ED25519_PUBLIC_KEY: &str = "ed25519_public_key";
const NODES: &str = "nodes";

/// One per-node row of the topology answer (`INT-NET-070 / 6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEntry {
    /// The node's UUID from the static manifest.
    pub sender_id: [u8; 16],
    /// The node label, not normalized and limited in UTF-8 bytes.
    pub label: String,
    /// The node's canonical IPv4 or IPv6 literal.
    pub host: CanonicalHost,
    /// The node's TCP port.
    pub port: u16,
    /// The node's open X25519 public key.
    pub x25519_public_key: [u8; 32],
    /// The node's open Ed25519 public key.
    pub ed25519_public_key: [u8; 32],
}

impl NodeEntry {
    /// `[sender_id, label, host, port, x25519_public_key, ed25519_public_key]`.
    pub fn to_value(&self, max_label_bytes: usize) -> Result<Value, TopologyError> {
        if self.label.len() > max_label_bytes {
            return Err(TopologyError::LabelTooLong {
                actual: self.label.len(),
                limit: max_label_bytes,
            });
        }

        if self.port == 0 {
            return Err(TopologyError::PortOutOfRange(0));
        }

        Ok(Value::Array(vec![
            Value::Bytes(self.sender_id.to_vec()),
            Value::Text(self.label.clone()),
            Value::Text(self.host.as_str().to_owned()),
            Value::Uint(u64::from(self.port)),
            Value::Bytes(self.x25519_public_key.to_vec()),
            Value::Bytes(self.ed25519_public_key.to_vec()),
        ]))
    }

    /// Decodes and validates one six-position node row.
    pub fn from_value(value: &Value, max_label_bytes: usize) -> Result<Self, TopologyError> {
        let Value::Array(items) = value else {
            return Err(TopologyError::WrongNodeShape);
        };

        let [
            sender_id,
            label,
            host,
            port,
            x25519_public_key,
            ed25519_public_key,
        ] = items.as_slice()
        else {
            return Err(TopologyError::WrongNodeShape);
        };

        let sender_id = fixed_bytes::<16>(sender_id, SENDER_ID)?;

        let Value::Text(label) = label else {
            return Err(TopologyError::WrongType(LABEL));
        };
        if label.len() > max_label_bytes {
            return Err(TopologyError::LabelTooLong {
                actual: label.len(),
                limit: max_label_bytes,
            });
        }

        let Value::Text(host) = host else {
            return Err(TopologyError::WrongType(HOST));
        };
        let host = CanonicalHost::parse(host)?;

        let Value::Uint(number) = port else {
            return Err(TopologyError::WrongType(PORT));
        };
        let port = match u16::try_from(*number) {
            Ok(port) if port != 0 => port,
            _ => return Err(TopologyError::PortOutOfRange(*number)),
        };

        Ok(Self {
            sender_id,
            label: label.clone(),
            host,
            port,
            x25519_public_key: fixed_bytes::<32>(x25519_public_key, X25519_PUBLIC_KEY)?,
            ed25519_public_key: fixed_bytes::<32>(ed25519_public_key, ED25519_PUBLIC_KEY)?,
        })
    }
}

/// The successful topology answer arguments `[nodes]` (`INT-NET-070 / 6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyAnswer {
    /// Node rows in static-manifest order.
    pub nodes: Vec<NodeEntry>,
}

impl TopologyAnswer {
    /// `[nodes]` result arguments; preserves the given order (manifest order).
    pub fn to_args(&self, max_label_bytes: usize) -> Result<Vec<Value>, TopologyError> {
        let nodes = self
            .nodes
            .iter()
            .map(|node| node.to_value(max_label_bytes))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(vec![Value::Array(nodes)])
    }

    /// Reads `[nodes]` and validates every row.
    pub fn from_args(args: &[Value], max_label_bytes: usize) -> Result<Self, TopologyError> {
        let [nodes] = args else {
            return Err(TopologyError::WrongAnswerShape);
        };

        let Value::Array(nodes) = nodes else {
            return Err(TopologyError::WrongType(NODES));
        };

        let nodes = nodes
            .iter()
            .map(|node| NodeEntry::from_value(node, max_label_bytes))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { nodes })
    }
}

/// Reasons a topology answer is not valid for protocol version 1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum TopologyError {
    /// The result arguments are not a one-element array.
    #[error("topology answer must be a one-element array")]
    WrongAnswerShape,
    /// A node row is not a six-element array.
    #[error("node entry must be a six-element array")]
    WrongNodeShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// A byte string position has the wrong length.
    #[error("invalid byte-string length for {0}")]
    InvalidLength(&'static str),
    /// The port is not in `1..=65535`.
    #[error("port {0} is out of range")]
    PortOutOfRange(u64),
    /// The node label exceeds the configured UTF-8 byte limit.
    #[error("node label is {actual} bytes, limit is {limit}")]
    LabelTooLong {
        /// Actual UTF-8 byte length of the label.
        actual: usize,
        /// Configured maximum label length in UTF-8 bytes (`CFG-CL-031`).
        limit: usize,
    },
    /// The topology host is not a canonical IP literal.
    #[error("invalid host: {0}")]
    Host(#[from] HostError),
}

fn fixed_bytes<const N: usize>(
    value: &Value,
    field: &'static str,
) -> Result<[u8; N], TopologyError> {
    match value {
        Value::Bytes(bytes) => {
            <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| TopologyError::InvalidLength(field))
        }
        _ => Err(TopologyError::WrongType(field)),
    }
}
