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

#[cfg(test)]
mod tests {
    use super::RaftRpc;
    use crate::cbor::Value;
    use crate::inter_server::MessageError;

    #[test]
    fn round_trips_opaque_rpc_bytes() {
        let bytes = vec![0x00, 0x7F, 0xFF];
        let args = RaftRpc {
            rpc_bytes: bytes.clone(),
        }
        .args();
        assert_eq!(RaftRpc::from_args(&args), Ok(RaftRpc { rpc_bytes: bytes }));
    }

    #[test]
    fn rejects_wrong_rpc_argument_count() {
        assert_eq!(
            RaftRpc::from_args(&[Value::Bytes(Vec::new()), Value::Null]),
            Err(MessageError::WrongArgumentCount {
                expected: 1,
                actual: 2,
            })
        );
    }

    #[test]
    fn rejects_non_byte_string_rpc_argument() {
        assert_eq!(
            RaftRpc::from_args(&[Value::Text("rpc".to_owned())]),
            Err(MessageError::WrongType("rpc_bytes"))
        );
    }
}
