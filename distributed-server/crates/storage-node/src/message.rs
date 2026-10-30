//! Inter-server message envelope for Raft RPCs.
//!
//! SRS: `INT-NET-072 / 2`, `INT-NET-074 / 2`, `INT-NET-075 / 1`,
//! `INT-NET-098 / 1`, `INT-NET-099 / 1`.

use ciborium::value::Value;

/// Assigned inter-server message-type codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    /// Dedicated liveness heartbeat request.
    DedicatedHeartbeatRequest = 1,
    /// Dedicated liveness heartbeat response.
    DedicatedHeartbeatResponse = 2,
    /// OpenRaft RPC.
    RaftRpc = 3,
    /// Blob put begin.
    BlobPutBegin = 10,
    /// Blob put data.
    BlobPutData = 11,
    /// Blob put result.
    BlobPutResult = 12,
    /// Blob get request.
    BlobGet = 13,
    /// Blob get result.
    BlobGetResult = 14,
}

impl TryFrom<u32> for MessageType {
    type Error = MessageError;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        match code {
            0 => Err(MessageError::ReservedMessageType),
            1 => Ok(Self::DedicatedHeartbeatRequest),
            2 => Ok(Self::DedicatedHeartbeatResponse),
            3 => Ok(Self::RaftRpc),
            10 => Ok(Self::BlobPutBegin),
            11 => Ok(Self::BlobPutData),
            12 => Ok(Self::BlobPutResult),
            13 => Ok(Self::BlobGet),
            14 => Ok(Self::BlobGetResult),
            code => Err(MessageError::UnknownMessageType { code }),
        }
    }
}

/// Errors from encoding or decoding an inter-server message envelope.
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    /// The decoded CBOR value is not an array.
    #[error("the message is not a CBOR array")]
    NotAnArray,
    /// The envelope does not contain exactly two elements.
    #[error("the message envelope must have exactly two elements")]
    EnvelopeLength,
    /// The message-type integer is outside the supported range.
    #[error("the message type is outside the supported range")]
    MessageTypeOutOfRange,
    /// The reserved code `0` was used.
    #[error("message type 0 is reserved")]
    ReservedMessageType,
    /// The code is not assigned in this protocol version.
    #[error("unknown message type {code}")]
    UnknownMessageType { code: u32 },
    /// A non-Raft message type reached the Raft RPC decoder.
    #[error("message type {message_type:?} is not a Raft RPC")]
    UnexpectedMessageType { message_type: MessageType },
    /// The arguments are not exactly one CBOR byte string.
    #[error("the arguments must be an array of exactly one CBOR byte string")]
    Arguments,
    /// The CBOR envelope could not be encoded.
    #[error("the message envelope could not be encoded")]
    Encode,
    /// The CBOR envelope could not be decoded, or trailing bytes followed it.
    #[error("the message envelope could not be decoded")]
    Decode,
}

/// Encodes a Raft RPC byte string as the inter-server `[3, [rpc_bytes]]` envelope.
pub fn encode_raft_rpc(rpc_bytes: &[u8]) -> Result<Vec<u8>, MessageError> {
    let envelope = Value::Array(vec![
        Value::Integer((MessageType::RaftRpc as u32).into()),
        Value::Array(vec![Value::Bytes(rpc_bytes.to_vec())]),
    ]);

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&envelope, &mut bytes).map_err(|_| MessageError::Encode)?;
    Ok(bytes)
}

/// Decodes an inter-server `[3, [rpc_bytes]]` envelope and returns the RPC bytes unchanged.
pub fn decode_raft_rpc(bytes: &[u8]) -> Result<Vec<u8>, MessageError> {
    let mut slice = bytes;
    let envelope: Value =
        ciborium::de::from_reader(&mut slice).map_err(|_| MessageError::Decode)?;
    if !slice.is_empty() {
        return Err(MessageError::Decode);
    }

    let Value::Array(elements) = envelope else {
        return Err(MessageError::NotAnArray);
    };
    let [message_type, arguments] = elements
        .try_into()
        .map_err(|_| MessageError::EnvelopeLength)?;

    let Value::Integer(code) = message_type else {
        return Err(MessageError::MessageTypeOutOfRange);
    };
    let code = u32::try_from(code).map_err(|_| MessageError::MessageTypeOutOfRange)?;
    let message_type = MessageType::try_from(code)?;
    if message_type != MessageType::RaftRpc {
        return Err(MessageError::UnexpectedMessageType { message_type });
    }

    let Value::Array(mut arguments) = arguments else {
        return Err(MessageError::Arguments);
    };
    if arguments.len() != 1 {
        return Err(MessageError::Arguments);
    }
    let Some(Value::Bytes(rpc_bytes)) = arguments.pop() else {
        return Err(MessageError::Arguments);
    };

    Ok(rpc_bytes)
}
