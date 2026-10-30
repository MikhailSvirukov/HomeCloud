//! Encrypted signed inter-server message composition.
//!
//! SRS: `INT-NET-019 / 1`, `INT-NET-020 / 1`, `INT-NET-021 / 3`,
//! `INT-NET-024 / 2`, `INT-NET-027 / 1`, `INT-NET-028 / 1`,
//! `INT-NET-037 / 1`, `INT-NET-072 / 2`, `INT-NET-074 / 2`,
//! `INT-NET-081 / 1`, `NFR-SEC-009 / 1`.

use core::num::NonZeroU32;
use std::fmt;

use crate::catalog_exchange::ExternalHeader;
use crate::client_limits::ClientLimits;
use crate::sealing::{open, seal, RecipientPrivateKey, RecipientPublicKey, SealingError};
use crate::signed_frame::{decode_signed_frame, encode_signed_frame, SignedFrameError};
use crate::signing::{MessageSigningKey, MessageVerifyingKey};

type Result<T> = core::result::Result<T, ServerExchangeError>;

/// Decrypted inter-server message with its verified external header.
#[derive(PartialEq, Eq)]
pub struct ServerMessage {
    /// Verified external header of the message frame.
    pub header: ExternalHeader,
    /// Decrypted logical-header and payload bytes.
    pub plaintext: Vec<u8>,
}

impl fmt::Debug for ServerMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerMessage")
            .field("header", &self.header)
            .field("plaintext_len", &self.plaintext.len())
            .finish()
    }
}

/// Errors from composing or opening an encrypted signed inter-server message.
#[derive(Debug, thiserror::Error)]
pub enum ServerExchangeError {
    /// The message frame is invalid or its signature did not verify.
    #[error(transparent)]
    Frame(#[from] SignedFrameError),
    /// The message payload could not be sealed or opened.
    #[error(transparent)]
    Sealing(#[from] SealingError),
}

/// Seals the payload for the recipient and encodes it inside a signed frame.
pub fn encode_server_message(
    header: &ExternalHeader,
    plaintext: &[u8],
    recipient: &RecipientPublicKey,
    signing_key: &MessageSigningKey,
    max_frame_len: NonZeroU32,
    limits: &ClientLimits,
) -> Result<Vec<u8>> {
    let sealed = seal(recipient, &[], &[], plaintext)?;
    Ok(encode_signed_frame(
        header,
        &sealed,
        signing_key,
        max_frame_len,
        limits,
    )?)
}

/// Verifies the frame signature, then opens the sealed payload for the recipient.
pub fn decode_server_message(
    bytes: &[u8],
    sender: &MessageVerifyingKey,
    recipient: &RecipientPrivateKey,
    max_frame_len: NonZeroU32,
    limits: &ClientLimits,
) -> Result<ServerMessage> {
    let frame = decode_signed_frame(bytes, sender, max_frame_len, limits)?;
    let plaintext = open(recipient, &[], &[], &frame.payload)?;
    Ok(ServerMessage {
        header: frame.header,
        plaintext,
    })
}
