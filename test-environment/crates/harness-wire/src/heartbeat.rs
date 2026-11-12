//! Dedicated liveness heartbeat request and response messages.
//!
//! Codes `1` and `2` carry `[sequence_number, dedicated_sent_at_ms,
//! previous_clock_offset_ms]` and `[sequence_number, node_received_at_ms,
//! node_sent_at_ms]` respectively (`INT-NET-073 / 6`).

use crate::cbor::Value;
use crate::inter_server::{MessageError, decode_int64, decode_uint64, encode_int64};

const SEQUENCE_NUMBER: &str = "sequence_number";
const DEDICATED_SENT_AT_MS: &str = "dedicated_sent_at_ms";
const PREVIOUS_CLOCK_OFFSET_MS: &str = "previous_clock_offset_ms";
const NODE_RECEIVED_AT_MS: &str = "node_received_at_ms";
const NODE_SENT_AT_MS: &str = "node_sent_at_ms";
const ARGUMENT_COUNT: usize = 3;

/// Dedicated liveness heartbeat request (`message_type = 1`, `INT-NET-073 / 6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatRequest {
    /// Heartbeat sequence number.
    pub sequence_number: u64,
    /// Unix time in milliseconds when the dedicated server sent the request.
    pub dedicated_sent_at_ms: i64,
    /// Previous calculated clock offset, or `None` before the first estimate.
    pub previous_clock_offset_ms: Option<i64>,
}

impl HeartbeatRequest {
    /// `[sequence_number, dedicated_sent_at_ms, previous_clock_offset_ms]`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::Uint(self.sequence_number),
            encode_int64(self.dedicated_sent_at_ms),
            self.previous_clock_offset_ms
                .map_or(Value::Null, encode_int64),
        ]
    }

    /// Decodes the three request arguments.
    pub fn from_args(args: &[Value]) -> Result<Self, MessageError> {
        let [
            sequence_number,
            dedicated_sent_at_ms,
            previous_clock_offset_ms,
        ] = args
        else {
            return Err(MessageError::WrongArgumentCount {
                expected: ARGUMENT_COUNT,
                actual: args.len(),
            });
        };

        let sequence_number = decode_uint64(sequence_number, SEQUENCE_NUMBER)?;
        let dedicated_sent_at_ms = decode_int64(dedicated_sent_at_ms, DEDICATED_SENT_AT_MS)?;
        let previous_clock_offset_ms = match previous_clock_offset_ms {
            Value::Null => None,
            value => Some(decode_int64(value, PREVIOUS_CLOCK_OFFSET_MS)?),
        };

        Ok(Self {
            sequence_number,
            dedicated_sent_at_ms,
            previous_clock_offset_ms,
        })
    }
}

/// Dedicated liveness heartbeat response (`message_type = 2`, `INT-NET-073 / 6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatResponse {
    /// The request's sequence number, returned unchanged.
    pub sequence_number: u64,
    /// Unix time in milliseconds when the node received the request.
    pub node_received_at_ms: i64,
    /// Unix time in milliseconds when the node sent the response.
    pub node_sent_at_ms: i64,
}

impl HeartbeatResponse {
    /// `[sequence_number, node_received_at_ms, node_sent_at_ms]`.
    pub fn args(&self) -> Vec<Value> {
        vec![
            Value::Uint(self.sequence_number),
            encode_int64(self.node_received_at_ms),
            encode_int64(self.node_sent_at_ms),
        ]
    }

    /// Decodes the three response arguments.
    pub fn from_args(args: &[Value]) -> Result<Self, MessageError> {
        let [sequence_number, node_received_at_ms, node_sent_at_ms] = args else {
            return Err(MessageError::WrongArgumentCount {
                expected: ARGUMENT_COUNT,
                actual: args.len(),
            });
        };

        Ok(Self {
            sequence_number: decode_uint64(sequence_number, SEQUENCE_NUMBER)?,
            node_received_at_ms: decode_int64(node_received_at_ms, NODE_RECEIVED_AT_MS)?,
            node_sent_at_ms: decode_int64(node_sent_at_ms, NODE_SENT_AT_MS)?,
        })
    }

    /// Builds the response that echoes the request's sequence number unchanged.
    pub fn reply_to(
        request: &HeartbeatRequest,
        node_received_at_ms: i64,
        node_sent_at_ms: i64,
    ) -> Self {
        Self {
            sequence_number: request.sequence_number,
            node_received_at_ms,
            node_sent_at_ms,
        }
    }
}
