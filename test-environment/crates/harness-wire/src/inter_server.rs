//! Inter-server message envelope and message-type registry.
//!
//! The decrypted logical header of an inter-server message is the positional
//! CBOR array `[message_type, args]` (`INT-NET-072 / 2`). Message-type codes and
//! their argument schemas are fixed by `INT-NET-073 / 6`, `INT-NET-074 / 2`, and
//! `INT-NET-075 / 1`.

use crate::cbor::{self, Value};
use crate::heartbeat::{HeartbeatRequest, HeartbeatResponse};
use crate::raft::RaftRpc;

/// The reserved `message_type` value.
pub const MESSAGE_TYPE_RESERVED: u32 = 0;
/// The dedicated liveness heartbeat request code.
pub const MESSAGE_TYPE_HEARTBEAT_REQUEST: u32 = 1;
/// The dedicated liveness heartbeat response code.
pub const MESSAGE_TYPE_HEARTBEAT_RESPONSE: u32 = 2;
/// The RPC OpenRaft message code.
pub const MESSAGE_TYPE_RPC_OPENRAFT: u32 = 3;

const MESSAGE_TYPE_FIELD: &str = "message_type";
const ARGS_FIELD: &str = "args";

/// A v1 inter-server message type (`INT-NET-075 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Dedicated liveness heartbeat request, code `1`.
    HeartbeatRequest,
    /// Dedicated liveness heartbeat response, code `2`.
    HeartbeatResponse,
    /// RPC OpenRaft wrapper, code `3`.
    RpcOpenRaft,
}

impl MessageType {
    /// The published `uint32` code.
    pub const fn code(self) -> u32 {
        match self {
            Self::HeartbeatRequest => MESSAGE_TYPE_HEARTBEAT_REQUEST,
            Self::HeartbeatResponse => MESSAGE_TYPE_HEARTBEAT_RESPONSE,
            Self::RpcOpenRaft => MESSAGE_TYPE_RPC_OPENRAFT,
        }
    }

    /// Resolves a published code; code `0` is reserved and other codes are
    /// unknown.
    pub fn from_code(code: u32) -> Result<Self, MessageError> {
        match code {
            MESSAGE_TYPE_HEARTBEAT_REQUEST => Ok(Self::HeartbeatRequest),
            MESSAGE_TYPE_HEARTBEAT_RESPONSE => Ok(Self::HeartbeatResponse),
            MESSAGE_TYPE_RPC_OPENRAFT => Ok(Self::RpcOpenRaft),
            MESSAGE_TYPE_RESERVED => Err(MessageError::ReservedMessageType),
            _ => Err(MessageError::UnknownMessageType(code)),
        }
    }
}

/// Reasons an inter-server message is not valid for protocol version 1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MessageError {
    /// The value is not a two-element CBOR array.
    #[error("inter-server envelope must be a two-element array")]
    WrongEnvelopeShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// The `message_type` code is reserved and rejected.
    #[error("message type 0 is reserved and rejected")]
    ReservedMessageType,
    /// The `message_type` code is not assigned in v1.
    #[error("unknown message type {0}")]
    UnknownMessageType(u32),
    /// A numeric argument does not fit its published range.
    #[error("{0} is out of range")]
    OutOfRange(&'static str),
    /// The argument array length does not match the message type.
    #[error("expected {expected} message arguments, got {actual}")]
    WrongArgumentCount {
        /// Number of arguments required by the message type.
        expected: usize,
        /// Number of arguments present in the decoded array.
        actual: usize,
    },
}

/// A decoded `[message_type, args]` inter-server message (`INT-NET-072 / 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterServerMessage {
    /// Dedicated liveness heartbeat request.
    HeartbeatRequest(HeartbeatRequest),
    /// Dedicated liveness heartbeat response.
    HeartbeatResponse(HeartbeatResponse),
    /// RPC OpenRaft wrapper.
    RaftRpc(RaftRpc),
}

impl InterServerMessage {
    /// The message type of this message.
    pub const fn message_type(&self) -> MessageType {
        match self {
            Self::HeartbeatRequest(_) => MessageType::HeartbeatRequest,
            Self::HeartbeatResponse(_) => MessageType::HeartbeatResponse,
            Self::RaftRpc(_) => MessageType::RpcOpenRaft,
        }
    }

    /// Encodes `[message_type, args]`; typed values always satisfy their schema.
    pub fn encode(&self) -> Vec<u8> {
        let args = match self {
            Self::HeartbeatRequest(message) => message.args(),
            Self::HeartbeatResponse(message) => message.args(),
            Self::RaftRpc(message) => message.args(),
        };
        let value = Value::Array(vec![
            Value::Uint(u64::from(self.message_type().code())),
            Value::Array(args),
        ]);
        cbor::encode(&value)
    }

    /// Decodes the envelope and strictly validates the arguments for the type.
    pub fn decode(value: &Value) -> Result<Self, MessageError> {
        let Value::Array(items) = value else {
            return Err(MessageError::WrongEnvelopeShape);
        };

        let [message_type, args] = items.as_slice() else {
            return Err(MessageError::WrongEnvelopeShape);
        };

        let Value::Uint(code) = message_type else {
            return Err(MessageError::WrongType(MESSAGE_TYPE_FIELD));
        };
        let code =
            u32::try_from(*code).map_err(|_| MessageError::OutOfRange(MESSAGE_TYPE_FIELD))?;

        let message_type = MessageType::from_code(code)?;
        let Value::Array(args) = args else {
            return Err(MessageError::WrongType(ARGS_FIELD));
        };

        match message_type {
            MessageType::HeartbeatRequest => {
                HeartbeatRequest::from_args(args).map(Self::HeartbeatRequest)
            }
            MessageType::HeartbeatResponse => {
                HeartbeatResponse::from_args(args).map(Self::HeartbeatResponse)
            }
            MessageType::RpcOpenRaft => RaftRpc::from_args(args).map(Self::RaftRpc),
        }
    }
}

/// The v1 message-type registry (`INT-NET-075 / 1`).
pub const V1_REGISTRY: [(u32, MessageType); 3] = [
    (
        MESSAGE_TYPE_HEARTBEAT_REQUEST,
        MessageType::HeartbeatRequest,
    ),
    (
        MESSAGE_TYPE_HEARTBEAT_RESPONSE,
        MessageType::HeartbeatResponse,
    ),
    (MESSAGE_TYPE_RPC_OPENRAFT, MessageType::RpcOpenRaft),
];

/// Reasons a published message-type table is not valid for v1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The table assigns the reserved code `0`.
    #[error("message type 0 is reserved and must be rejected")]
    ReservedCode,
    /// The table assigns a code that v1 does not define.
    #[error("message type code {0} is not assigned in v1")]
    UnassignedCode(u32),
    /// The table assigns the same code more than once.
    #[error("message type code {0} is assigned more than once")]
    DuplicateCode(u32),
    /// The table assigns a code to the wrong message type.
    #[error("message type code {code} is assigned to {actual:?}, not {expected:?}")]
    MismatchedAssignment {
        /// The conflicting code.
        code: u32,
        /// Message type published for the code.
        expected: MessageType,
        /// Message type the table assigns to the code.
        actual: MessageType,
    },
}

/// Validates a published message-type table: code 0 reserved, every code
/// assigned in v1, no duplicate codes, and no code/type conflict.
pub fn validate_registry(entries: &[(u32, MessageType)]) -> Result<(), RegistryError> {
    let mut seen = Vec::new();

    for &(code, message_type) in entries {
        if code == MESSAGE_TYPE_RESERVED {
            return Err(RegistryError::ReservedCode);
        }

        let Some(expected) = assigned_message_type(code) else {
            return Err(RegistryError::UnassignedCode(code));
        };

        if seen.contains(&code) {
            return Err(RegistryError::DuplicateCode(code));
        }
        seen.push(code);

        if message_type != expected {
            return Err(RegistryError::MismatchedAssignment {
                code,
                expected,
                actual: message_type,
            });
        }
    }

    Ok(())
}

fn assigned_message_type(code: u32) -> Option<MessageType> {
    V1_REGISTRY
        .iter()
        .find(|(assigned, _)| *assigned == code)
        .map(|(_, message_type)| *message_type)
}

/// Encodes an `int64` field as a non-negative unsigned integer or a negative
/// integer, matching the deterministic CBOR representation.
pub(crate) fn encode_int64(number: i64) -> Value {
    match u64::try_from(number) {
        Ok(non_negative) => Value::Uint(non_negative),
        Err(_) => Value::Int(number),
    }
}

/// Decodes a `uint64` field; a negative CBOR integer is out of range.
pub(crate) fn decode_uint64(value: &Value, field: &'static str) -> Result<u64, MessageError> {
    match value {
        Value::Uint(number) => Ok(*number),
        Value::Int(number) if *number < 0 => Err(MessageError::OutOfRange(field)),
        _ => Err(MessageError::WrongType(field)),
    }
}

/// Decodes an `int64` field from an unsigned integer that fits or a negative
/// CBOR integer.
pub(crate) fn decode_int64(value: &Value, field: &'static str) -> Result<i64, MessageError> {
    match value {
        Value::Uint(number) => i64::try_from(*number).map_err(|_| MessageError::OutOfRange(field)),
        Value::Int(number) => Ok(*number),
        _ => Err(MessageError::WrongType(field)),
    }
}

/// Decodes a CBOR byte string without copying it.
pub(crate) fn decode_bytes<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a [u8], MessageError> {
    match value {
        Value::Bytes(bytes) => Ok(bytes.as_slice()),
        _ => Err(MessageError::WrongType(field)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InterServerMessage, MESSAGE_TYPE_HEARTBEAT_REQUEST, MESSAGE_TYPE_HEARTBEAT_RESPONSE,
        MESSAGE_TYPE_RESERVED, MESSAGE_TYPE_RPC_OPENRAFT, MessageError, MessageType, RegistryError,
        V1_REGISTRY, validate_registry,
    };
    use crate::cbor::{self, Value};
    use crate::heartbeat::{HeartbeatRequest, HeartbeatResponse};
    use crate::raft::RaftRpc;

    #[test]
    fn resolves_published_message_type_codes() {
        assert_eq!(
            MessageType::from_code(MESSAGE_TYPE_HEARTBEAT_REQUEST),
            Ok(MessageType::HeartbeatRequest)
        );
        assert_eq!(
            MessageType::from_code(MESSAGE_TYPE_HEARTBEAT_RESPONSE),
            Ok(MessageType::HeartbeatResponse)
        );
        assert_eq!(
            MessageType::from_code(MESSAGE_TYPE_RPC_OPENRAFT),
            Ok(MessageType::RpcOpenRaft)
        );
    }

    #[test]
    fn rejects_reserved_message_type_code() {
        assert_eq!(
            MessageType::from_code(MESSAGE_TYPE_RESERVED),
            Err(MessageError::ReservedMessageType)
        );
    }

    #[test]
    fn rejects_unknown_message_type_codes() {
        assert_eq!(
            MessageType::from_code(4),
            Err(MessageError::UnknownMessageType(4))
        );
        assert_eq!(
            MessageType::from_code(u32::MAX),
            Err(MessageError::UnknownMessageType(u32::MAX))
        );
    }

    #[test]
    fn accepts_v1_registry() {
        assert_eq!(validate_registry(&V1_REGISTRY), Ok(()));
    }

    #[test]
    fn rejects_reserved_registry_code() {
        assert_eq!(
            validate_registry(&[(MESSAGE_TYPE_RESERVED, MessageType::HeartbeatRequest)]),
            Err(RegistryError::ReservedCode)
        );
    }

    #[test]
    fn rejects_unassigned_registry_code() {
        assert_eq!(
            validate_registry(&[(4, MessageType::HeartbeatRequest)]),
            Err(RegistryError::UnassignedCode(4))
        );
    }

    #[test]
    fn rejects_duplicate_registry_code() {
        assert_eq!(
            validate_registry(&[
                (
                    MESSAGE_TYPE_HEARTBEAT_REQUEST,
                    MessageType::HeartbeatRequest
                ),
                (
                    MESSAGE_TYPE_HEARTBEAT_REQUEST,
                    MessageType::HeartbeatRequest
                ),
            ]),
            Err(RegistryError::DuplicateCode(MESSAGE_TYPE_HEARTBEAT_REQUEST))
        );
    }

    #[test]
    fn rejects_conflicting_registry_assignment() {
        assert_eq!(
            validate_registry(&[(
                MESSAGE_TYPE_HEARTBEAT_RESPONSE,
                MessageType::HeartbeatRequest,
            )]),
            Err(RegistryError::MismatchedAssignment {
                code: MESSAGE_TYPE_HEARTBEAT_RESPONSE,
                expected: MessageType::HeartbeatResponse,
                actual: MessageType::HeartbeatRequest,
            })
        );
    }

    #[test]
    fn round_trips_heartbeat_request_envelope() {
        let message = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
            sequence_number: 7,
            dedicated_sent_at_ms: -1,
            previous_clock_offset_ms: Some(0),
        });
        let value = cbor::decode(&message.encode()).expect("envelope is valid CBOR");
        assert_eq!(InterServerMessage::decode(&value), Ok(message));
    }

    #[test]
    fn round_trips_heartbeat_response_envelope() {
        let message = InterServerMessage::HeartbeatResponse(HeartbeatResponse {
            sequence_number: 7,
            node_received_at_ms: 10,
            node_sent_at_ms: 20,
        });
        let value = cbor::decode(&message.encode()).expect("envelope is valid CBOR");
        assert_eq!(InterServerMessage::decode(&value), Ok(message));
    }

    #[test]
    fn round_trips_raft_rpc_envelope() {
        let message = InterServerMessage::RaftRpc(RaftRpc {
            rpc_bytes: vec![0x00, 0x7F, 0xFF],
        });
        let value = cbor::decode(&message.encode()).expect("envelope is valid CBOR");
        assert_eq!(InterServerMessage::decode(&value), Ok(message));
    }

    #[test]
    fn rejects_envelope_with_wrong_shape() {
        assert_eq!(
            InterServerMessage::decode(&Value::Array(vec![Value::Uint(1)])),
            Err(MessageError::WrongEnvelopeShape)
        );
        assert_eq!(
            InterServerMessage::decode(&Value::Null),
            Err(MessageError::WrongEnvelopeShape)
        );
    }
}
