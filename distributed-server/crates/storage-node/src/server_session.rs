//! Incoming encrypted peer session.
//!
//! SRS: `FR-CL-001 / 1`, `FR-CL-031 / 1`, `INT-NET-021 / 3`,
//! `INT-NET-024 / 2`, `INT-NET-027 / 1`, `INT-NET-037 / 1`,
//! `INT-NET-072 / 2`, `INT-NET-074 / 2`, `INT-NET-081 / 1`,
//! `NFR-SEC-009 / 1`.

use core::num::NonZeroU32;
use std::future::Future;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::catalog_exchange::{
    ExternalHeader, EXTERNAL_PROTOCOL_VERSION, REQUEST_ID_LEN, SENDER_ID_LEN,
};
use crate::client_limits::ClientLimits;
use crate::framing::{FrameLength, FRAME_LENGTH_PREFIX_LEN};
use crate::raft::RaftError;
use crate::sealing::{RecipientPrivateKey, SealingError};
use crate::server_exchange::{decode_server_message, encode_server_message, ServerExchangeError};
use crate::server_transport::PeerDirectory;
use crate::signed_frame::{peek_external_header, SignedFrameError};
use crate::signing::MessageSigningKey;

type Result<T> = core::result::Result<T, ServerSessionError>;

/// Errors from serving an incoming encrypted peer session.
#[derive(Debug, thiserror::Error)]
pub enum ServerSessionError {
    /// The peer frame is invalid or its signature did not verify.
    #[error(transparent)]
    Frame(#[from] SignedFrameError),
    /// The sealed peer payload could not be opened.
    #[error(transparent)]
    Sealing(#[from] SealingError),
    /// The peer message could not be composed or opened.
    #[error(transparent)]
    Exchange(#[from] ServerExchangeError),
    /// The sender is not in the trusted peer directory.
    #[error("the sender is not in the trusted peer directory")]
    UnknownSender,
    /// The peer header is not an ordinary correlated request.
    #[error("the peer header is not an ordinary correlated request")]
    InvalidHeader,
    /// The local responder label is invalid.
    #[error("the local responder label is invalid")]
    InvalidLabel,
    /// The RPC handler failed.
    #[error("the RPC handler failed")]
    Handler(#[source] Box<RaftError>),
    /// The peer session I/O failed.
    #[error("the peer session I/O failed")]
    Io(#[from] io::Error),
}

/// Serves one encrypted peer request and writes its signed, sealed reply.
#[expect(
    clippy::too_many_arguments,
    reason = "the session inputs are independent explicit configuration"
)]
pub async fn serve_peer_connection<S, H, F>(
    mut stream: S,
    local_sender_id: [u8; SENDER_ID_LEN],
    local_label: &str,
    signing_key: &MessageSigningKey,
    recipient_key: &RecipientPrivateKey,
    peers: &PeerDirectory,
    max_frame_len: NonZeroU32,
    limits: &ClientLimits,
    handler: H,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: FnOnce(Vec<u8>) -> F,
    F: Future<Output = core::result::Result<Vec<u8>, RaftError>>,
{
    if !limits.responder_label_allowed(local_label) {
        return Err(ServerSessionError::InvalidLabel);
    }

    let frame = read_frame(&mut stream, max_frame_len).await?;
    let peeked = peek_external_header(&frame, max_frame_len, limits)?;
    let sender_id = request_sender(&peeked)?;

    let peer = peers
        .by_sender(sender_id)
        .ok_or(ServerSessionError::UnknownSender)?;
    let message = decode_server_message(
        &frame,
        &peer.ed25519_public_key,
        recipient_key,
        max_frame_len,
        limits,
    )?;
    if !matches_request(&message.header, peer.sender_id, peeked.request_id) {
        return Err(ServerSessionError::InvalidHeader);
    }

    let reply = handler(message.plaintext)
        .await
        .map_err(|error| ServerSessionError::Handler(Box::new(error)))?;

    let response_header = ExternalHeader {
        protocol_version: EXTERNAL_PROTOCOL_VERSION,
        sender_id: Some(local_sender_id),
        request_id: peeked.request_id,
        frame_number: 0,
        upload_id: None,
        responder_label: Some(local_label.to_owned()),
        bootstrap_signing_public_key: None,
    };
    let response = encode_server_message(
        &response_header,
        &reply,
        &peer.x25519_public_key,
        signing_key,
        max_frame_len,
        limits,
    )?;
    stream.write_all(&response).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_frame<S>(stream: &mut S, max_frame_len: NonZeroU32) -> Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut prefix = [0u8; FRAME_LENGTH_PREFIX_LEN];
    stream.read_exact(&mut prefix).await?;
    let length = FrameLength::decode(prefix, max_frame_len).map_err(SignedFrameError::Framing)?;
    let mut frame = vec![0u8; FRAME_LENGTH_PREFIX_LEN + length.get() as usize];
    frame[..FRAME_LENGTH_PREFIX_LEN].copy_from_slice(&prefix);
    stream
        .read_exact(&mut frame[FRAME_LENGTH_PREFIX_LEN..])
        .await?;
    Ok(frame)
}

fn request_sender(header: &ExternalHeader) -> Result<[u8; SENDER_ID_LEN]> {
    let Some(sender_id) = header.sender_id else {
        return Err(ServerSessionError::InvalidHeader);
    };
    if header.upload_id.is_some()
        || header.bootstrap_signing_public_key.is_some()
        || header.frame_number != 0
    {
        return Err(ServerSessionError::InvalidHeader);
    }
    Ok(sender_id)
}

fn matches_request(
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
