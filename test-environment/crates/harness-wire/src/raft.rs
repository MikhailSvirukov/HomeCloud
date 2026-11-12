//! RPC OpenRaft wrapper message (`message_type = 3`).
//!
//! Each RPC is carried as `[rpc_bytes]`, where the bytes are opaque to this
//! layer and are returned unchanged (`INT-NET-074 / 2`).

use crate::cbor::Value;
use crate::inter_server::{MessageError, decode_bytes};

const RPC_BYTES: &str = "rpc_bytes";
const ARGUMENT_COUNT: usize = 1;

/// RPC OpenRaft wrapper (`message_type = 3`, `INT-NET-074 / 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaftRpc {
    /// Serialized RPC bytes produced by the OpenRaft transport adapter.
    pub rpc_bytes: Vec<u8>,
}

impl RaftRpc {
    /// `[rpc_bytes]`.
    pub fn args(&self) -> Vec<Value> {
        vec![Value::Bytes(self.rpc_bytes.clone())]
    }

    /// Decodes the single opaque byte-string argument.
    pub fn from_args(args: &[Value]) -> Result<Self, MessageError> {
        let [rpc_bytes] = args else {
            return Err(MessageError::WrongArgumentCount {
                expected: ARGUMENT_COUNT,
                actual: args.len(),
            });
        };

        Ok(Self {
            rpc_bytes: decode_bytes(rpc_bytes, RPC_BYTES)?.to_vec(),
        })
    }
}
