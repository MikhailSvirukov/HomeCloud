//! Direct encrypted TCP transport for inter-server Raft RPCs.
//!
//! SRS: `FR-CL-001 / 1`, `INT-NET-021 / 3`, `INT-NET-024 / 2`,
//! `INT-NET-027 / 1`, `INT-NET-037 / 1`, `INT-NET-072 / 2`,
//! `INT-NET-074 / 2`, `INT-NET-081 / 1`, `NFR-SEC-009 / 1`.

use core::fmt;
use core::num::NonZeroU32;
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::catalog_exchange::{
    ExternalHeader, EXTERNAL_PROTOCOL_VERSION, REQUEST_ID_LEN, SENDER_ID_LEN,
};
use crate::client_limits::ClientLimits;
use crate::framing::{FrameLength, FRAME_LENGTH_PREFIX_LEN};
use crate::raft::{NodeId, RaftTransport, RaftTransportError};
use crate::sealing::{RecipientPrivateKey, RecipientPublicKey, SealingError};
use crate::server_exchange::{decode_server_message, encode_server_message};
use crate::signing::{MessageSigningKey, MessageVerifyingKey, SigningError};

type TransportResult<T> = core::result::Result<T, RaftTransportError>;

/// Trusted peer node with the keys used to exchange encrypted Raft RPCs.
#[derive(Debug)]
pub struct Peer {
    /// Raft node identifier.
    pub node_id: NodeId,
    /// Sender identifier advertised in the external header.
    pub sender_id: [u8; 16],
    /// TCP address of the peer.
    pub address: SocketAddr,
    /// X25519 public key used to seal frames to the peer.
    pub x25519_public_key: RecipientPublicKey,
    /// Ed25519 public key used to verify the peer's signatures.
    pub ed25519_public_key: MessageVerifyingKey,
}

impl Peer {
    /// Builds a trusted peer from its identifier, address, and public key bytes.
    pub fn new(
        node_id: NodeId,
        sender_id: [u8; 16],
        address: SocketAddr,
        x25519_public_key: &[u8],
        ed25519_public_key: [u8; 32],
    ) -> Result<Self, PeerError> {
        Ok(Self {
            node_id,
            sender_id,
            address,
            x25519_public_key: RecipientPublicKey::from_bytes(x25519_public_key)?,
            ed25519_public_key: MessageVerifyingKey::from_bytes(ed25519_public_key)?,
        })
    }
}

/// Failure modes when building a trusted peer.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    /// The peer X25519 public key is invalid.
    #[error("the peer X25519 public key is invalid")]
    X25519(#[from] SealingError),
    /// The peer Ed25519 public key is invalid.
    #[error("the peer Ed25519 public key is invalid")]
    Ed25519(#[from] SigningError),
}

/// Trusted peer table that resolves a Raft node id to its keys and address.
pub struct PeerDirectory {
    peers: BTreeMap<NodeId, Peer>,
}

impl PeerDirectory {
    /// Builds a directory, rejecting duplicate node ids and sender ids.
    pub fn new(peers: Vec<Peer>) -> Result<Self, PeerDirectoryError> {
        let mut directory = BTreeMap::new();
        let mut sender_ids = BTreeSet::new();
        for peer in peers {
            if directory.contains_key(&peer.node_id) {
                return Err(PeerDirectoryError::DuplicateNode {
                    node_id: peer.node_id,
                });
            }
            if !sender_ids.insert(peer.sender_id) {
                return Err(PeerDirectoryError::DuplicateSender);
            }
            directory.insert(peer.node_id, peer);
        }
        Ok(Self { peers: directory })
    }

    /// Returns the peer registered for `node_id`.
    pub fn get(&self, node_id: NodeId) -> Option<&Peer> {
        self.peers.get(&node_id)
    }
}

/// Failure modes when building a trusted peer table.
#[derive(Debug, thiserror::Error)]
pub enum PeerDirectoryError {
    /// The same node id appears more than once.
    #[error("duplicate peer node id {node_id}")]
    DuplicateNode {
        /// The duplicated node id.
        node_id: NodeId,
    },
    /// The same sender id appears more than once.
    #[error("duplicate peer sender id")]
    DuplicateSender,
}

/// Direct encrypted TCP transport for inter-server Raft RPCs.
#[derive(Clone)]
pub struct ServerTransport {
    sender_id: [u8; 16],
    signing_key: Arc<MessageSigningKey>,
    recipient_key: Arc<RecipientPrivateKey>,
    peers: Arc<PeerDirectory>,
    max_frame_len: NonZeroU32,
    limits: ClientLimits,
    connect_timeout: Duration,
}

impl ServerTransport {
    /// Builds a transport with a local sender id, key pair, trusted peers, and connection limits.
    pub fn new(
        sender_id: [u8; 16],
        signing_key: MessageSigningKey,
        recipient_key: RecipientPrivateKey,
        peers: Arc<PeerDirectory>,
        max_frame_len: NonZeroU32,
        limits: ClientLimits,
        connect_timeout: Duration,
    ) -> Self {
        Self {
            sender_id,
            signing_key: Arc::new(signing_key),
            recipient_key: Arc::new(recipient_key),
            peers,
            max_frame_len,
            limits,
            connect_timeout,
        }
    }

    async fn exchange(&self, target: NodeId, request: Vec<u8>) -> TransportResult<Vec<u8>> {
        let peer = self
            .peers
            .get(target)
            .ok_or(RaftTransportError::Unreachable { target })?;
        let request_id = random_request_id(target)?;
        let header = ExternalHeader {
            protocol_version: EXTERNAL_PROTOCOL_VERSION,
            sender_id: Some(self.sender_id),
            request_id,
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        };
        let frame = encode_server_message(
            &header,
            &request,
            &peer.x25519_public_key,
            &self.signing_key,
            self.max_frame_len,
            &self.limits,
        )
        .map_err(|_| send_error(target))?;

        let reply_frame = self.round_trip(peer.address, &frame, target).await?;
        let reply = decode_server_message(
            &reply_frame,
            &peer.ed25519_public_key,
            &self.recipient_key,
            self.max_frame_len,
            &self.limits,
        )
        .map_err(|_| send_error(target))?;
        if !reply_matches(&reply.header, peer.sender_id, request_id) {
            return Err(send_error(target));
        }
        Ok(reply.plaintext)
    }

    async fn round_trip(
        &self,
        address: SocketAddr,
        frame: &[u8],
        target: NodeId,
    ) -> TransportResult<Vec<u8>> {
        let mut stream = tokio::time::timeout(self.connect_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| RaftTransportError::Unreachable { target })?
            .map_err(|_| RaftTransportError::Unreachable { target })?;

        stream
            .write_all(frame)
            .await
            .map_err(|_| send_error(target))?;

        let mut prefix = [0u8; FRAME_LENGTH_PREFIX_LEN];
        stream
            .read_exact(&mut prefix)
            .await
            .map_err(|_| send_error(target))?;
        let length =
            FrameLength::decode(prefix, self.max_frame_len).map_err(|_| send_error(target))?;
        let mut body = vec![0u8; length.get() as usize];
        stream
            .read_exact(&mut body)
            .await
            .map_err(|_| send_error(target))?;

        let mut reply = Vec::with_capacity(FRAME_LENGTH_PREFIX_LEN + body.len());
        reply.extend_from_slice(&prefix);
        reply.extend_from_slice(&body);
        Ok(reply)
    }
}

impl RaftTransport for ServerTransport {
    async fn send(&self, target: NodeId, request: Vec<u8>) -> Result<Vec<u8>, RaftTransportError> {
        self.exchange(target, request).await
    }
}

impl fmt::Debug for ServerTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerTransport")
            .field("sender_id", &self.sender_id)
            .field("peer_count", &self.peers.peers.len())
            .finish()
    }
}

fn random_request_id(target: NodeId) -> TransportResult<[u8; REQUEST_ID_LEN]> {
    let mut request_id = [0u8; REQUEST_ID_LEN];
    getrandom::fill(&mut request_id).map_err(|_| send_error(target))?;
    Ok(request_id)
}

fn reply_matches(
    header: &ExternalHeader,
    sender_id: [u8; SENDER_ID_LEN],
    request_id: [u8; REQUEST_ID_LEN],
) -> bool {
    header.sender_id == Some(sender_id)
        && header.request_id == request_id
        && header.frame_number == 0
        && header.upload_id.is_none()
        && header.bootstrap_signing_public_key.is_none()
}

fn send_error(target: NodeId) -> RaftTransportError {
    RaftTransportError::Send { target }
}
