//! Ed25519 signing and verification for network messages.
//!
//! SRS: `INT-NET-018 / 1`, `INT-NET-020 / 1`, `INT-NET-028 / 1`.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

/// Length of an Ed25519 private or public key in bytes.
pub const SIGNING_KEY_LEN: usize = 32;
/// Length of an Ed25519 signature in bytes.
pub const SIGNATURE_LEN: usize = 64;

/// Failure modes when verifying a message signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SigningError {
    /// The signature did not verify for the message and key.
    #[error("invalid message signature")]
    InvalidSignature,
    /// The bytes are not a valid Ed25519 verifying key.
    #[error("invalid Ed25519 verifying key")]
    InvalidVerifyingKey,
}

type Result<T> = core::result::Result<T, SigningError>;

/// Ed25519 key used to sign outgoing messages.
pub struct MessageSigningKey(SigningKey);

impl MessageSigningKey {
    /// Builds from the 32 private-key bytes loaded from the configured key file.
    pub fn from_bytes(bytes: [u8; SIGNING_KEY_LEN]) -> Self {
        Self(SigningKey::from_bytes(&bytes))
    }

    /// Signs `message`, returning the 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.0.sign(message).to_bytes()
    }

    /// Returns the matching public verifying key.
    pub fn verifying_key(&self) -> MessageVerifyingKey {
        MessageVerifyingKey(self.0.verifying_key())
    }
}

impl fmt::Debug for MessageSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MessageSigningKey { key: <redacted> }")
    }
}

/// Ed25519 public key used to verify incoming messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageVerifyingKey(VerifyingKey);

impl MessageVerifyingKey {
    /// Parses the 32 public-key bytes.
    pub fn from_bytes(bytes: [u8; SIGNING_KEY_LEN]) -> Result<Self> {
        VerifyingKey::from_bytes(&bytes)
            .map(Self)
            .map_err(|_| SigningError::InvalidVerifyingKey)
    }

    /// Verifies `signature` over `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8; SIGNATURE_LEN]) -> Result<()> {
        self.0
            .verify_strict(message, &Signature::from_bytes(signature))
            .map_err(|_| SigningError::InvalidSignature)
    }

    /// Returns the 32 public-key bytes.
    pub fn as_bytes(&self) -> [u8; SIGNING_KEY_LEN] {
        self.0.to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_key_debug_redacts_key_bytes() {
        let bytes = [0xAB; SIGNING_KEY_LEN];
        let key = MessageSigningKey::from_bytes(bytes);
        let rendered = format!("{key:?}");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();

        assert!(rendered.contains("redacted"));
        assert!(!rendered.contains(&hex));
    }
}
