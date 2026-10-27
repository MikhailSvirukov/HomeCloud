//! Public API integration tests for the inter-server message envelope.
//!
//! SRS: `INT-NET-072 / 2` (the positional `[message_type, args]` CBOR
//! envelope), `INT-NET-074 / 2` (Raft RPCs travel as `message_type = 3` with a
//! single CBOR byte string), `INT-NET-075 / 1` (the message-type code registry
//! with a reserved `0`), supporting `FR-CL-031 / 1` (the local OpenRaft instance
//! receives the RPC bytes unchanged).

use storage_node::{decode_raft_rpc, encode_raft_rpc, MessageError, MessageType};

const RPC_BYTES: [u8; 3] = [0x01, 0x02, 0x03];
const GOLDEN_ENVELOPE: [u8; 7] = [0x82, 0x03, 0x81, 0x43, 0x01, 0x02, 0x03];
const LARGE_RPC_LEN: usize = 300;

#[test]
fn encodes_the_raft_rpc_envelope_as_golden_bytes() {
    let envelope = encode_raft_rpc(&RPC_BYTES).expect("the Raft RPC envelope must encode");

    assert_eq!(envelope, GOLDEN_ENVELOPE);
}

#[test]
fn decodes_the_golden_envelope() {
    let rpc_bytes = decode_raft_rpc(&GOLDEN_ENVELOPE).expect("the golden envelope must decode");

    assert_eq!(rpc_bytes, RPC_BYTES);
}

#[test]
fn round_trips_opaque_rpc_bytes() {
    let cases = [
        Vec::new(),
        vec![0x00, 0xff, 0x80],
        vec![0xa5; LARGE_RPC_LEN],
    ];

    for rpc_bytes in cases {
        let envelope = encode_raft_rpc(&rpc_bytes).expect("the Raft RPC envelope must encode");
        let decoded = decode_raft_rpc(&envelope).expect("the Raft RPC envelope must decode");

        assert_eq!(decoded, rpc_bytes);
    }
}

#[test]
fn rejects_the_reserved_message_type() {
    let result = decode_raft_rpc(&[0x82, 0x00, 0x81, 0x40]);

    assert!(matches!(result, Err(MessageError::ReservedMessageType)));
}

#[test]
fn rejects_an_unknown_message_type() {
    let result = decode_raft_rpc(&[0x82, 0x04, 0x81, 0x40]);

    assert!(matches!(
        result,
        Err(MessageError::UnknownMessageType { code: 4 })
    ));
}

#[test]
fn rejects_a_non_raft_message_type() {
    let result = decode_raft_rpc(&[0x82, 0x01, 0x81, 0x40]);

    assert!(matches!(
        result,
        Err(MessageError::UnexpectedMessageType {
            message_type: MessageType::DedicatedHeartbeatRequest
        })
    ));
}

#[test]
fn rejects_a_wrong_envelope_shape() {
    let not_an_array = decode_raft_rpc(&[0x03]);
    let wrong_length = decode_raft_rpc(&[0x81, 0x03]);

    assert!(matches!(not_an_array, Err(MessageError::NotAnArray)));
    assert!(matches!(wrong_length, Err(MessageError::EnvelopeLength)));
}

#[test]
fn rejects_wrong_arguments() {
    let non_array_arguments = decode_raft_rpc(&[0x82, 0x03, 0x03]);
    let non_byte_string_argument = decode_raft_rpc(&[0x82, 0x03, 0x81, 0x03]);
    let too_many_arguments = decode_raft_rpc(&[0x82, 0x03, 0x82, 0x40, 0x40]);

    assert!(matches!(non_array_arguments, Err(MessageError::Arguments)));
    assert!(matches!(
        non_byte_string_argument,
        Err(MessageError::Arguments)
    ));
    assert!(matches!(too_many_arguments, Err(MessageError::Arguments)));
}

#[test]
fn rejects_a_non_integer_or_out_of_range_message_type() {
    let non_integer = decode_raft_rpc(&[0x82, 0x40, 0x81, 0x40]);
    let negative = decode_raft_rpc(&[0x82, 0x20, 0x81, 0x40]);

    assert!(matches!(
        non_integer,
        Err(MessageError::MessageTypeOutOfRange)
    ));
    assert!(matches!(negative, Err(MessageError::MessageTypeOutOfRange)));
}

#[test]
fn rejects_trailing_bytes() {
    let mut trailing = GOLDEN_ENVELOPE.to_vec();
    trailing.push(0x00);

    let result = decode_raft_rpc(&trailing);

    assert!(matches!(result, Err(MessageError::Decode)));
}

#[test]
fn maps_the_assigned_message_type_codes() {
    let heartbeat_request =
        MessageType::try_from(1).expect("code 1 is the dedicated heartbeat request");
    let heartbeat_response =
        MessageType::try_from(2).expect("code 2 is the dedicated heartbeat response");
    let raft_rpc = MessageType::try_from(3).expect("code 3 is the OpenRaft RPC");

    assert_eq!(heartbeat_request, MessageType::DedicatedHeartbeatRequest);
    assert_eq!(heartbeat_response, MessageType::DedicatedHeartbeatResponse);
    assert_eq!(raft_rpc, MessageType::RaftRpc);
    assert!(matches!(
        MessageType::try_from(0),
        Err(MessageError::ReservedMessageType)
    ));
    assert!(matches!(
        MessageType::try_from(4),
        Err(MessageError::UnknownMessageType { code }) if code == 4
    ));
    assert!(matches!(
        MessageType::try_from(u32::MAX),
        Err(MessageError::UnknownMessageType { code }) if code == u32::MAX
    ));
}
