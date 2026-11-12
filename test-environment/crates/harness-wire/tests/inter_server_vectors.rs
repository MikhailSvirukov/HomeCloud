//! Golden vectors for the v1 inter-server envelope and message types.
//!
//! Every test pins exact bytes from [`contracts/wire-v1.md`] §8 so component
//! implementations can consume these vectors as external conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::cbor::{self, Value};
use harness_wire::inter_server::{MESSAGE_TYPE_HEARTBEAT_REQUEST, MESSAGE_TYPE_RPC_OPENRAFT};
use harness_wire::{
    HeartbeatRequest, HeartbeatResponse, InterServerMessage, MessageError, MessageType, RaftRpc,
    RegistryError, V1_REGISTRY, validate_registry,
};

/// Builds a `[message_type, args]` envelope for decode inputs.
fn envelope(message_type: Value, args: Value) -> Value {
    Value::Array(vec![message_type, args])
}

/// Decodes a golden vector and then the inter-server message it carries.
fn decode_message(bytes: &[u8]) -> Result<InterServerMessage, MessageError> {
    let value = cbor::decode(bytes).expect("golden vector is valid CBOR");
    InterServerMessage::decode(&value)
}

/// `INT-NET-072/2`, `INT-NET-073/6`, `TST-036/5`.
#[test]
fn heartbeat_request_matches_contract_vector() {
    let message = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: 1,
        dedicated_sent_at_ms: 1000,
        previous_clock_offset_ms: Some(-5),
    });
    let expected = vec![0x82, 0x01, 0x83, 0x01, 0x19, 0x03, 0xE8, 0x24];

    assert_eq!(message.encode(), expected);
    assert_eq!(decode_message(&expected), Ok(message));
}

/// `INT-NET-073/6`, `TST-036/5`: `previous_clock_offset_ms` is `null` before
/// the first calculated estimate.
#[test]
fn first_heartbeat_request_has_null_offset() {
    let message = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: 1,
        dedicated_sent_at_ms: 1000,
        previous_clock_offset_ms: None,
    });

    assert_eq!(
        message.encode(),
        vec![0x82, 0x01, 0x83, 0x01, 0x19, 0x03, 0xE8, 0xF6]
    );
}

/// `INT-NET-087`, `TST-036/5`: a calculated offset of zero is CBOR integer
/// zero, not `null`.
#[test]
fn calculated_zero_clock_offset_is_integer_zero() {
    let message = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: 1,
        dedicated_sent_at_ms: 1000,
        previous_clock_offset_ms: Some(0),
    });

    assert_eq!(
        message.encode(),
        vec![0x82, 0x01, 0x83, 0x01, 0x19, 0x03, 0xE8, 0x00]
    );
}

/// `INT-NET-087`, `TST-036/5`: a positive calculated offset is a positive CBOR
/// integer, distinct from `null`, zero, and negative offsets.
#[test]
fn positive_clock_offset_uses_minimal_integer() {
    let message = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: 1,
        dedicated_sent_at_ms: 1000,
        previous_clock_offset_ms: Some(5),
    });
    let expected = vec![0x82, 0x01, 0x83, 0x01, 0x19, 0x03, 0xE8, 0x05];

    assert_eq!(message.encode(), expected);
    assert_eq!(decode_message(&expected), Ok(message));
}

/// `INT-NET-073/6`, `TST-036/5`.
#[test]
fn heartbeat_response_matches_contract_vector() {
    let message = InterServerMessage::HeartbeatResponse(HeartbeatResponse {
        sequence_number: 1,
        node_received_at_ms: 2000,
        node_sent_at_ms: 3000,
    });
    let expected = vec![0x82, 0x02, 0x83, 0x01, 0x19, 0x07, 0xD0, 0x19, 0x0B, 0xB8];

    assert_eq!(message.encode(), expected);
    assert_eq!(decode_message(&expected), Ok(message));
}

/// `INT-NET-073/6`, `TST-036/5`: the response returns the received sequence
/// number unchanged.
#[test]
fn heartbeat_response_echoes_the_request_sequence() {
    let request = HeartbeatRequest {
        sequence_number: 1,
        dedicated_sent_at_ms: 1000,
        previous_clock_offset_ms: None,
    };

    let response = HeartbeatResponse::reply_to(&request, 2000, 3000);

    assert_eq!(response.sequence_number, request.sequence_number);
    assert_eq!(
        InterServerMessage::HeartbeatResponse(response).encode(),
        vec![0x82, 0x02, 0x83, 0x01, 0x19, 0x07, 0xD0, 0x19, 0x0B, 0xB8]
    );
}

/// `INT-NET-073/6`, `TST-036/5`: both response timestamps use the negative
/// `int64` path down to `i64::MIN`.
#[test]
fn heartbeat_response_accepts_negative_int64_timestamps() {
    let message = InterServerMessage::HeartbeatResponse(HeartbeatResponse {
        sequence_number: 1,
        node_received_at_ms: -1,
        node_sent_at_ms: i64::MIN,
    });
    let expected = vec![
        0x82, 0x02, 0x83, 0x01, 0x20, 0x3B, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];

    assert_eq!(message.encode(), expected);
    assert_eq!(decode_message(&expected), Ok(message));
}

/// `INT-NET-073/6`, `TST-036/5`: every heartbeat number uses the minimal
/// deterministic encoding, including the `uint64` maximum.
#[test]
fn heartbeat_numbers_use_minimal_widths() {
    let maximum_sequence = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: u64::MAX,
        dedicated_sent_at_ms: 0,
        previous_clock_offset_ms: None,
    });
    assert_eq!(
        maximum_sequence.encode(),
        vec![
            0x82, 0x01, 0x83, 0x1B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0xF6,
        ]
    );

    let negative_numbers = InterServerMessage::HeartbeatRequest(HeartbeatRequest {
        sequence_number: 0,
        dedicated_sent_at_ms: -1,
        previous_clock_offset_ms: Some(-1),
    });
    assert_eq!(
        negative_numbers.encode(),
        vec![0x82, 0x01, 0x83, 0x00, 0x20, 0x20]
    );
}

/// `INT-NET-074/2`, `TST-037/2`: the RPC wrapper is `[3, [rpc_bytes]]`.
#[test]
fn raft_rpc_matches_contract_vector() {
    let message = InterServerMessage::RaftRpc(RaftRpc {
        rpc_bytes: vec![0xDE, 0xAD, 0xBE, 0xEF],
    });
    assert_eq!(
        message.encode(),
        vec![0x82, 0x03, 0x81, 0x44, 0xDE, 0xAD, 0xBE, 0xEF]
    );

    let empty = InterServerMessage::RaftRpc(RaftRpc { rpc_bytes: vec![] });
    assert_eq!(empty.encode(), vec![0x82, 0x03, 0x81, 0x40]);
}

/// `INT-NET-074/2`, `TST-037/2`: `rpc_bytes` is opaque and survives the
/// wrapper unchanged; signing and encryption stay outside this crate.
#[test]
fn raft_rpc_preserves_opaque_bytes() {
    let rpc_bytes = vec![0x00, 0x7F, 0xFF, 0x00, 0x01];
    let message = InterServerMessage::RaftRpc(RaftRpc {
        rpc_bytes: rpc_bytes.clone(),
    });

    assert_eq!(decode_message(&message.encode()), Ok(message));
    assert_eq!(
        RaftRpc::from_args(&[Value::Bytes(rpc_bytes.clone())]),
        Ok(RaftRpc { rpc_bytes })
    );
}

/// `INT-NET-075/1`, `TST-035/2`, `TST-038/1`: code `0` is reserved and
/// unassigned codes are rejected.
#[test]
fn envelope_rejects_reserved_and_unknown_message_types() {
    assert_eq!(
        InterServerMessage::decode(&envelope(Value::Uint(0), Value::Array(vec![]))),
        Err(MessageError::ReservedMessageType)
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(Value::Uint(4), Value::Array(vec![]))),
        Err(MessageError::UnknownMessageType(4))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(u32::MAX)),
            Value::Array(vec![])
        )),
        Err(MessageError::UnknownMessageType(u32::MAX))
    );
}

/// `INT-NET-072/2`, `TST-035/2`: the envelope is exactly two elements, the
/// code is a `uint32`, and the arguments are an array.
#[test]
fn envelope_rejects_wrong_shape_and_types() {
    assert_eq!(
        InterServerMessage::decode(&Value::Array(vec![
            Value::Uint(1),
            Value::Array(vec![]),
            Value::Null,
        ])),
        Err(MessageError::WrongEnvelopeShape)
    );
    assert_eq!(
        InterServerMessage::decode(&Value::Null),
        Err(MessageError::WrongEnvelopeShape)
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(Value::Text("1".to_owned()), Value::Array(vec![]))),
        Err(MessageError::WrongType("message_type"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(Value::Uint(1), Value::Uint(0))),
        Err(MessageError::WrongType("args"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(u32::MAX) + 1),
            Value::Array(vec![])
        )),
        Err(MessageError::OutOfRange("message_type"))
    );
}

/// `INT-NET-073/6`, `INT-NET-074/2`, `TST-035/2`: each message type fixes its
/// exact argument count.
#[test]
fn heartbeat_rejects_wrong_argument_count() {
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![Value::Uint(1), Value::Uint(1000)]),
        )),
        Err(MessageError::WrongArgumentCount {
            expected: 3,
            actual: 2,
        })
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![
                Value::Uint(1),
                Value::Uint(1000),
                Value::Null,
                Value::Null,
            ]),
        )),
        Err(MessageError::WrongArgumentCount {
            expected: 3,
            actual: 4,
        })
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_RPC_OPENRAFT)),
            Value::Array(vec![Value::Bytes(vec![]), Value::Null]),
        )),
        Err(MessageError::WrongArgumentCount {
            expected: 1,
            actual: 2,
        })
    );
}

/// `INT-NET-073/6`, `TST-036/5`: heartbeat fields keep their published CBOR
/// types and `int64`/`uint64` ranges.
#[test]
fn heartbeat_rejects_wrong_argument_types() {
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![
                Value::Text("1".to_owned()),
                Value::Uint(1000),
                Value::Null,
            ]),
        )),
        Err(MessageError::WrongType("sequence_number"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![Value::Int(-1), Value::Uint(1000), Value::Null]),
        )),
        Err(MessageError::OutOfRange("sequence_number"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![
                Value::Uint(1),
                Value::Text("1000".to_owned()),
                Value::Null,
            ]),
        )),
        Err(MessageError::WrongType("dedicated_sent_at_ms"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![Value::Uint(1), Value::Uint(u64::MAX), Value::Null]),
        )),
        Err(MessageError::OutOfRange("dedicated_sent_at_ms"))
    );
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_HEARTBEAT_REQUEST)),
            Value::Array(vec![
                Value::Uint(1),
                Value::Uint(1000),
                Value::Text("-5".to_owned()),
            ]),
        )),
        Err(MessageError::WrongType("previous_clock_offset_ms"))
    );
}

/// `INT-NET-074/2`, `TST-037/2`: the single RPC argument is a byte string.
#[test]
fn raft_rpc_rejects_wrong_argument() {
    assert_eq!(
        InterServerMessage::decode(&envelope(
            Value::Uint(u64::from(MESSAGE_TYPE_RPC_OPENRAFT)),
            Value::Array(vec![Value::Text("rpc".to_owned())]),
        )),
        Err(MessageError::WrongType("rpc_bytes"))
    );
}

/// `INT-NET-075/1`, `TST-038/1`: the published registry assigns codes `1`, `2`,
/// and `3` exactly once.
#[test]
fn v1_registry_matches_contract_table() {
    assert_eq!(
        V1_REGISTRY,
        [
            (1, MessageType::HeartbeatRequest),
            (2, MessageType::HeartbeatResponse),
            (3, MessageType::RpcOpenRaft),
        ]
    );
    assert_eq!(validate_registry(&V1_REGISTRY), Ok(()));
}

/// `INT-NET-075/1`, `TST-038/1`: code `0`, unassigned codes, duplicates, and
/// conflicting assignments are rejected.
#[test]
fn registry_rejects_reserved_duplicate_and_conflicting_codes() {
    assert_eq!(
        validate_registry(&[(0, MessageType::HeartbeatRequest)]),
        Err(RegistryError::ReservedCode)
    );
    assert_eq!(
        validate_registry(&[(4, MessageType::HeartbeatRequest)]),
        Err(RegistryError::UnassignedCode(4))
    );
    assert_eq!(
        validate_registry(&[
            (1, MessageType::HeartbeatRequest),
            (1, MessageType::HeartbeatRequest),
        ]),
        Err(RegistryError::DuplicateCode(1))
    );
    assert_eq!(
        validate_registry(&[(2, MessageType::HeartbeatRequest)]),
        Err(RegistryError::MismatchedAssignment {
            code: 2,
            expected: MessageType::HeartbeatResponse,
            actual: MessageType::HeartbeatRequest,
        })
    );
}
