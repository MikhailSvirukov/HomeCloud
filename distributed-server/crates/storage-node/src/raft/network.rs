//! OpenRaft network adapter and the CBOR RPC codec.

#![expect(
    clippy::result_large_err,
    reason = "the RPC adapter forwards openraft's error types unchanged"
)]

use openraft::error::InstallSnapshotError;
use openraft::error::NetworkError;
use openraft::error::RPCError;
use openraft::error::RaftError as OpenraftError;
use openraft::error::RemoteError;
use openraft::network::RPCOption;
use openraft::network::RaftNetwork;
use openraft::network::RaftNetworkFactory;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::InstallSnapshotRequest;
use openraft::raft::InstallSnapshotResponse;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;
use openraft::BasicNode;

use super::NodeId;
use super::RaftConfig;
use super::RaftError;
use super::RaftTransport;

/// One encoded Raft RPC carried inside the inter-server `rpc_bytes` payload.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum RaftRpc {
    AppendEntries(AppendEntriesRequest<RaftConfig>),
    Vote(VoteRequest<NodeId>),
    InstallSnapshot(InstallSnapshotRequest<RaftConfig>),
}

/// Encodes a Raft message as CBOR (RFC 8949).
pub(super) fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, RaftError> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).map_err(|_| RaftError::Encode)?;
    Ok(bytes)
}

/// Decodes a Raft message from CBOR (RFC 8949).
pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, RaftError> {
    ciborium::de::from_reader(bytes).map_err(|_| RaftError::Decode)
}

/// Raft network factory that forwards every RPC through a [`RaftTransport`].
#[derive(Clone)]
pub(super) struct Network<T> {
    transport: T,
    target: NodeId,
}

/// The transport did not answer within the RPC hard deadline.
#[derive(Debug, thiserror::Error)]
#[error("the RPC to peer {target} timed out")]
struct RpcTimeout {
    target: NodeId,
}

impl<T: RaftTransport> Network<T> {
    pub(super) const fn new(transport: T) -> Self {
        Self {
            transport,
            // The factory replaces this placeholder with the real peer id.
            target: 0,
        }
    }

    async fn send_rpc<Resp, Err>(
        &self,
        rpc: RaftRpc,
        option: RPCOption,
    ) -> Result<Resp, RPCError<NodeId, BasicNode, Err>>
    where
        Resp: serde::de::DeserializeOwned,
        Err: std::error::Error + serde::de::DeserializeOwned,
    {
        let request = encode(&rpc).map_err(|error| RPCError::Network(NetworkError::new(&error)))?;
        let request = crate::message::encode_raft_rpc(&request)
            .map_err(|error| RPCError::Network(NetworkError::new(&error)))?;
        let reply =
            tokio::time::timeout(option.hard_ttl(), self.transport.send(self.target, request))
                .await
                .map_err(|_| {
                    RPCError::Network(NetworkError::new(&RpcTimeout {
                        target: self.target,
                    }))
                })?
                .map_err(|error| RPCError::Network(NetworkError::new(&error)))?;
        let reply = crate::message::decode_raft_rpc(&reply)
            .map_err(|error| RPCError::Network(NetworkError::new(&error)))?;
        let result: Result<Resp, Err> =
            decode(&reply).map_err(|error| RPCError::Network(NetworkError::new(&error)))?;
        result.map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }
}

impl<T: RaftTransport> RaftNetworkFactory<RaftConfig> for Network<T> {
    type Network = Network<T>;

    async fn new_client(&mut self, target: NodeId, _node: &BasicNode) -> Self::Network {
        Self {
            transport: self.transport.clone(),
            target,
        }
    }
}

impl<T: RaftTransport> RaftNetwork<RaftConfig> for Network<T> {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<RaftConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, BasicNode, OpenraftError<NodeId>>>
    {
        self.send_rpc(RaftRpc::AppendEntries(rpc), option).await
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, BasicNode, OpenraftError<NodeId>>> {
        self.send_rpc(RaftRpc::Vote(rpc), option).await
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<RaftConfig>,
        option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, BasicNode, OpenraftError<NodeId, InstallSnapshotError>>,
    > {
        self.send_rpc(RaftRpc::InstallSnapshot(rpc), option).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use openraft::CommittedLeaderId;
    use openraft::Entry;
    use openraft::EntryPayload;
    use openraft::LogId;
    use openraft::SnapshotMeta;
    use openraft::StoredMembership;
    use openraft::Vote;

    use super::*;
    use crate::metadata::ContentHash;
    use crate::metadata::UploadId;
    use crate::metadata::UploadRecord;
    use crate::metadata::CONTENT_HASH_LEN;
    use crate::metadata::UPLOAD_ID_LEN;
    use crate::raft::CommittedVersion;
    use crate::raft::RaftRequest;
    use crate::raft::RaftResponse;

    fn log_id() -> LogId<NodeId> {
        LogId::new(CommittedLeaderId::new(2, 1), 3)
    }

    fn upload_id() -> UploadId {
        UploadId::from_bytes([4; UPLOAD_ID_LEN])
    }

    fn content_hash() -> ContentHash {
        ContentHash::from_bytes([0xab; CONTENT_HASH_LEN])
    }

    fn committed_version() -> CommittedVersion {
        CommittedVersion {
            record: UploadRecord::new(
                upload_id(),
                "alice",
                "original.bin",
                1_024,
                content_hash(),
                1_700_000_000_000,
            )
            .expect("the test record is valid"),
            replicas: BTreeSet::from([1, 2]),
        }
    }

    #[test]
    fn append_entries_rpc_round_trips_through_cbor() {
        let request = RaftRpc::AppendEntries(AppendEntriesRequest {
            vote: Vote::new(2, 1),
            prev_log_id: Some(log_id()),
            entries: vec![Entry {
                log_id: log_id(),
                payload: EntryPayload::Normal(RaftRequest::CommitVersion(committed_version())),
            }],
            leader_commit: Some(log_id()),
        });

        let decoded: RaftRpc =
            decode(&encode(&request).expect("the RPC encodes")).expect("the RPC decodes");
        let RaftRpc::AppendEntries(decoded) = decoded else {
            panic!("expected an append-entries RPC");
        };
        assert_eq!(decoded.vote, Vote::new(2, 1));
        assert_eq!(decoded.prev_log_id, Some(log_id()));
        assert_eq!(decoded.leader_commit, Some(log_id()));
        assert_eq!(decoded.entries.len(), 1);
    }

    #[test]
    fn vote_rpc_round_trips_through_cbor() {
        let request = RaftRpc::Vote(VoteRequest {
            vote: Vote::new(5, 2),
            last_log_id: Some(log_id()),
        });

        let decoded: RaftRpc =
            decode(&encode(&request).expect("the RPC encodes")).expect("the RPC decodes");
        let RaftRpc::Vote(decoded) = decoded else {
            panic!("expected a vote RPC");
        };
        assert_eq!(
            decoded,
            VoteRequest {
                vote: Vote::new(5, 2),
                last_log_id: Some(log_id()),
            }
        );
    }

    #[test]
    fn install_snapshot_rpc_round_trips_through_cbor() {
        let request = RaftRpc::InstallSnapshot(InstallSnapshotRequest {
            vote: Vote::new(6, 2),
            meta: SnapshotMeta {
                last_log_id: Some(log_id()),
                last_membership: StoredMembership::default(),
                snapshot_id: "snapshot-1".to_owned(),
            },
            offset: 0,
            data: vec![1, 2, 3],
            done: true,
        });

        let decoded: RaftRpc =
            decode(&encode(&request).expect("the RPC encodes")).expect("the RPC decodes");
        let RaftRpc::InstallSnapshot(decoded) = decoded else {
            panic!("expected an install-snapshot RPC");
        };
        assert_eq!(decoded.data, vec![1, 2, 3]);
        assert!(decoded.done);
    }

    #[test]
    fn application_request_and_response_round_trip_through_cbor() {
        let request = RaftRequest::CommitVersion(CommittedVersion {
            record: UploadRecord::new(
                upload_id(),
                "alice",
                "original.bin",
                1_024,
                content_hash(),
                1_700_000_000_000,
            )
            .expect("the test record is valid"),
            replicas: BTreeSet::from([1, 2]),
        });
        let decoded: RaftRequest =
            decode(&encode(&request).expect("the request encodes")).expect("the request decodes");
        assert_eq!(decoded, request);

        let response = RaftResponse {
            upload_id: Some(upload_id()),
        };
        let decoded: RaftResponse = decode(&encode(&response).expect("the response encodes"))
            .expect("the response decodes");
        assert_eq!(
            decoded,
            RaftResponse {
                upload_id: Some(upload_id()),
            }
        );
    }
}
