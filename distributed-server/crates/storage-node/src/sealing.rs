//! HPKE sealing for inter-server messages.
//!
//! SRS: `INT-NET-027 / 1`, supporting `INT-NET-015 / 1`, `INT-NET-024 / 2`,
//! `INT-NET-033 / 1`, `NFR-SEC-003 / 1`, `NFR-SEC-006 / 1`.

use std::fmt;

use hpke::{
    aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, setup_receiver, setup_sender,
    Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable,
};

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = AesGcm256;

/// Length of an X25519 public or private key in bytes.
pub const X25519_KEY_LEN: usize = 32;
/// Length of an HPKE encapsulated key in bytes.
pub const ENCAPSULATED_KEY_LEN: usize = 32;

/// Failure modes when sealing or opening a message.
#[derive(Debug, thiserror::Error)]
pub enum SealingError {
    /// The X25519 public key encoding is invalid.
    #[error("the X25519 public key is invalid")]
    InvalidPublicKey,
    /// The X25519 private key encoding is invalid.
    #[error("the X25519 private key is invalid")]
    InvalidPrivateKey,
    /// The X25519 key does not have the required length.
    #[error("the X25519 key must be 32 bytes")]
    InvalidLength,
    /// The sealed message does not contain an encapsulated key.
    #[error("the sealed message is shorter than the encapsulated key")]
    Truncated,
    /// The message could not be encrypted for the recipient.
    #[error("the message could not be sealed")]
    Seal,
    /// The sealed message could not be decrypted for the recipient.
    #[error("the sealed message could not be opened")]
    Open,
}

type Result<T> = core::result::Result<T, SealingError>;

/// X25519 public key of a message recipient.
#[derive(Clone)]
pub struct RecipientPublicKey(<Kem as KemTrait>::PublicKey);

impl RecipientPublicKey {
    /// Parses the 32 public-key bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != X25519_KEY_LEN {
            return Err(SealingError::InvalidLength);
        }
        <Kem as KemTrait>::PublicKey::from_bytes(bytes)
            .map(Self)
            .map_err(|_| SealingError::InvalidPublicKey)
    }

    /// Returns the 32 public-key bytes.
    pub fn as_bytes(&self) -> [u8; X25519_KEY_LEN] {
        let mut bytes = [0u8; X25519_KEY_LEN];
        bytes.copy_from_slice(&self.0.to_bytes());
        bytes
    }
}

impl fmt::Debug for RecipientPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RecipientPublicKey")
            .field(&self.as_bytes())
            .finish()
    }
}

/// X25519 private key of a message recipient.
#[derive(Clone)]
pub struct RecipientPrivateKey(<Kem as KemTrait>::PrivateKey);

impl RecipientPrivateKey {
    /// Parses the 32 private-key bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != X25519_KEY_LEN {
            return Err(SealingError::InvalidLength);
        }
        <Kem as KemTrait>::PrivateKey::from_bytes(bytes)
            .map(Self)
            .map_err(|_| SealingError::InvalidPrivateKey)
    }

    /// Returns the matching public key.
    pub fn public_key(&self) -> RecipientPublicKey {
        RecipientPublicKey(<Kem as KemTrait>::sk_to_pk(&self.0))
    }
}

impl fmt::Debug for RecipientPrivateKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecipientPrivateKey { key: <redacted> }")
    }
}

/// Seals `plaintext` for `recipient`, returning the encapsulated key followed by the ciphertext.
pub fn seal(
    recipient: &RecipientPublicKey,
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let (encapsulated_key, mut context) =
        setup_sender::<Aead, Kdf, Kem>(&OpModeS::Base, &recipient.0, info)
            .map_err(|_| SealingError::Seal)?;
    let ciphertext = context
        .seal(plaintext, aad)
        .map_err(|_| SealingError::Seal)?;
    let mut sealed = Vec::with_capacity(ENCAPSULATED_KEY_LEN + ciphertext.len());
    sealed.extend_from_slice(&encapsulated_key.to_bytes());
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

/// Opens a sealed message for `recipient`, returning the plaintext.
pub fn open(
    recipient: &RecipientPrivateKey,
    info: &[u8],
    aad: &[u8],
    sealed: &[u8],
) -> Result<Vec<u8>> {
    if sealed.len() < ENCAPSULATED_KEY_LEN {
        return Err(SealingError::Truncated);
    }
    let (encapsulated_key, ciphertext) = sealed.split_at(ENCAPSULATED_KEY_LEN);
    let encapsulated_key = <Kem as KemTrait>::EncappedKey::from_bytes(encapsulated_key)
        .map_err(|_| SealingError::Open)?;
    let mut context =
        setup_receiver::<Aead, Kdf, Kem>(&OpModeR::Base, &recipient.0, &encapsulated_key, info)
            .map_err(|_| SealingError::Open)?;
    context
        .open(ciphertext, aad)
        .map_err(|_| SealingError::Open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_private_key_debug_redacts_key_bytes() {
        let bytes = [0xAB; X25519_KEY_LEN];
        let key = RecipientPrivateKey::from_bytes(&bytes).expect("full-length key is valid");
        let rendered = format!("{key:?}");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();

        assert!(rendered.contains("redacted"));
        assert!(!rendered.contains(&hex));
    }
}
