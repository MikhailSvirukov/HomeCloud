//! Node runtime: manifest-validated OpenRaft and encrypted peer RPC service.
//!
//! SRS: `FR-CL-001 / 1`, `FR-CL-015 / 1`, `FR-CL-017 / 1`, `FR-CL-027 / 1`,
//! `FR-CL-030 / 1`, `FR-CL-031 / 1`, `CFG-CL-003 / 1`, `CFG-CL-007 / 1`,
//! `CFG-CL-021 / 1`, `CFG-CL-029 / 1`, `CFG-CL-031 / 1`, `CFG-CL-039 / 1`,
//! `INT-NET-071 / 1`, `INT-NET-074 / 2`, `NFR-SEC-009 / 1`.

use core::fmt;
use core::num::NonZeroU32;
use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tracing::warn;

use crate::client_limits::ClientLimits;
use crate::keys::PrivateKey;
use crate::listener::{Listener, ListenerError};
use crate::manifest::{ManifestEntry, ManifestError, StaticManifest};
use crate::raft::{NodeId, RaftError, RaftNode, RaftTiming};
use crate::sealing::{RecipientPrivateKey, SealingError};
use crate::server_session::serve_peer_connection;
use crate::server_transport::{PeerDirectory, ServerTransport};
use crate::signing::MessageSigningKey;

/// Failure modes when starting or serving the node runtime.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// The static manifest is invalid or does not identify the local node.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The configured listen address is not a valid socket address.
    #[error("the configured listen address is invalid")]
    InvalidListenAddress,
    /// The configured listen address does not match the local manifest entry.
    #[error("the configured listen address does not match the local manifest address")]
    AddressMismatch,
    /// The configured X25519 key does not match the local manifest key.
    #[error("the configured X25519 key does not match the local manifest key")]
    X25519Mismatch,
    /// The configured Ed25519 key does not match the local manifest key.
    #[error("the configured Ed25519 key does not match the local manifest key")]
    Ed25519Mismatch,
    /// The local manifest responder label exceeds the configured byte limit.
    #[error("the local responder label is invalid")]
    InvalidLabel,
    /// The configured encryption key is not a valid X25519 private key.
    #[error(transparent)]
    Sealing(#[from] SealingError),
    /// The local Raft node could not be started or initialized.
    #[error("the local Raft node could not be started or initialized")]
    Raft(#[source] Box<RaftError>),
}

/// Manifest-validated node runtime that owns the local OpenRaft instance and
/// serves encrypted peer RPC sessions on an accepted TCP stream.
pub struct RaftRuntime {
    node: Arc<RaftNode>,
    local: ManifestEntry,
    peers: Arc<PeerDirectory>,
    signing_key: Arc<MessageSigningKey>,
    recipient_key: Arc<RecipientPrivateKey>,
    max_frame_len: NonZeroU32,
    limits: ClientLimits,
    members: BTreeMap<NodeId, String>,
}

impl RaftRuntime {
    /// Validates the local identity and keys against the static manifest, then
    /// starts the local OpenRaft instance backed by the encrypted peer transport.
    #[expect(
        clippy::too_many_arguments,
        reason = "the runtime inputs are independent explicit node configuration"
    )]
    pub async fn start(
        manifest: &StaticManifest,
        local_label: &str,
        listen_addr: &str,
        encryption_key: &PrivateKey,
        signing_key: &PrivateKey,
        max_frame_len: NonZeroU32,
        limits: &ClientLimits,
        storage_connect_timeout: Duration,
        raft_dir: PathBuf,
    ) -> Result<Self, RuntimeError> {
        let listen_addr = listen_addr
            .parse::<SocketAddr>()
            .map_err(|_| RuntimeError::InvalidListenAddress)?;
        let entry = manifest.local(local_label)?;
        let local = ManifestEntry {
            sender_id: entry.sender_id,
            label: entry.label.clone(),
            address: entry.address,
            x25519_public_key: entry.x25519_public_key,
            ed25519_public_key: entry.ed25519_public_key,
        };

        if listen_addr.port() != local.address.port() {
            return Err(RuntimeError::AddressMismatch);
        }
        if !listen_addr.ip().is_unspecified() && listen_addr.ip() != local.address.ip() {
            return Err(RuntimeError::AddressMismatch);
        }

        let signing = MessageSigningKey::from_bytes(*signing_key.as_bytes());
        if signing.verifying_key().as_bytes() != local.ed25519_public_key {
            return Err(RuntimeError::Ed25519Mismatch);
        }
        let recipient = RecipientPrivateKey::from_bytes(encryption_key.as_bytes())?;
        if recipient.public_key().as_bytes() != local.x25519_public_key {
            return Err(RuntimeError::X25519Mismatch);
        }
        if !limits.responder_label_allowed(&local.label) {
            return Err(RuntimeError::InvalidLabel);
        }

        let peers = Arc::new(manifest.peer_directory()?);
        let transport = ServerTransport::new(
            local.sender_id,
            MessageSigningKey::from_bytes(*signing_key.as_bytes()),
            RecipientPrivateKey::from_bytes(encryption_key.as_bytes())?,
            Arc::clone(&peers),
            max_frame_len,
            limits.clone(),
            storage_connect_timeout,
        );
        let node = RaftNode::start(local.node_id(), transport, RaftTiming::default(), raft_dir)
            .await
            .map_err(|error| RuntimeError::Raft(Box::new(error)))?;
        let members = manifest.voting_members()?;

        Ok(Self {
            node: Arc::new(node),
            local,
            peers,
            signing_key: Arc::new(signing),
            recipient_key: Arc::new(recipient),
            max_frame_len,
            limits: limits.clone(),
            members,
        })
    }

    /// Bootstraps the initial cluster membership with every storage node from
    /// the static manifest; a persisted bootstrap is rejected by the Raft layer.
    pub async fn initialize(&self) -> Result<(), RuntimeError> {
        self.node
            .initialize(self.members.clone())
            .await
            .map_err(|error| RuntimeError::Raft(Box::new(error)))
    }

    /// Returns the shared local OpenRaft instance.
    pub fn node(&self) -> Arc<RaftNode> {
        Arc::clone(&self.node)
    }

    /// Returns the validated local manifest entry.
    pub fn local(&self) -> &ManifestEntry {
        &self.local
    }

    /// Serves encrypted peer RPC sessions on `listener` until `shutdown`
    /// resolves, dispatching every verified request to the local OpenRaft node.
    pub async fn serve<F>(self, listener: Listener, shutdown: F) -> Result<(), ListenerError>
    where
        F: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        let Self {
            node,
            local,
            peers,
            signing_key,
            recipient_key,
            max_frame_len,
            limits,
            members: _,
        } = self;
        let ManifestEntry {
            sender_id: local_sender_id,
            label: local_label,
            ..
        } = local;
        let mut sessions = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept_stream() => {
                    let (stream, peer) = match accepted {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            stop_sessions(&mut sessions).await;
                            return Err(error);
                        }
                    };
                    let node = Arc::clone(&node);
                    let peers = Arc::clone(&peers);
                    let signing_key = Arc::clone(&signing_key);
                    let recipient_key = Arc::clone(&recipient_key);
                    let limits = limits.clone();
                    let local_label = local_label.clone();
                    sessions.spawn(async move {
                        let handler = move |request: Vec<u8>| {
                            let node = Arc::clone(&node);
                            async move { node.handle_rpc(&request).await }
                        };
                        let served = serve_peer_connection(
                            stream,
                            local_sender_id,
                            &local_label,
                            signing_key.as_ref(),
                            recipient_key.as_ref(),
                            peers.as_ref(),
                            max_frame_len,
                            &limits,
                            handler,
                        )
                        .await;
                        if let Err(error) = served {
                            warn!(peer = %peer, error = %error, "peer session failed");
                        }
                    });
                    while sessions.try_join_next().is_some() {}
                }
                () = &mut shutdown => {
                    stop_sessions(&mut sessions).await;
                    return Ok(());
                }
            }
        }
    }
}

impl fmt::Debug for RaftRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RaftRuntime")
            .field("sender_id", &self.local.sender_id)
            .field("member_count", &self.members.len())
            .finish()
    }
}

async fn stop_sessions(sessions: &mut JoinSet<()>) {
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
}
