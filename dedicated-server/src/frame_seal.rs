//! Sealing and opening of an application frame body (`INT-NET-081 / 1`).
//!
//! A frame is the four-byte big-endian length prefix, the encoded external
//! header, a 64-byte Ed25519 signature over the prefix, the header and the
//! payload, and finally the payload itself: the 32-byte HPKE encapsulated
//! X25519 key followed by the AES-256-GCM ciphertext and tag. The signature is
//! verified before any decryption, and every payload is encrypted under a fresh
//! encapsulation with an empty `info` and empty associated data.

use hpke::aead::AesGcm256;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, OpModeR, OpModeS, Serializable};
use ring::signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519};
use thiserror::Error;

/// Length of the big-endian body-length prefix.
pub const LENGTH_PREFIX_BYTES: usize = 4;
/// Length of the Ed25519 signature over prefix, header and payload.
pub const SIGNATURE_BYTES: usize = 64;
/// Length of the HPKE encapsulated X25519 key.
pub const ENCAPSULATED_KEY_BYTES: usize = 32;
/// Length of the AES-256-GCM authentication tag.
pub const AEAD_TAG_BYTES: usize = 16;
/// Length of a raw X25519 secret or public key.
pub const X25519_KEY_BYTES: usize = 32;
/// Length of an Ed25519 seed or public key.
pub const ED25519_KEY_BYTES: usize = 32;

/// Smallest payload section: encapsulated key plus authentication tag.
const MINIMUM_PAYLOAD_BYTES: usize = ENCAPSULATED_KEY_BYTES + AEAD_TAG_BYTES;

/// Errors produced while sealing or opening a frame.
#[derive(Debug, Error)]
pub enum FrameSealError {
    /// The frame is shorter than the fixed layout.
    #[error("the frame is {actual} bytes, below the {minimum}-byte minimum")]
    FrameTooShort {
        /// Observed frame length.
        actual: usize,
        /// Smallest possible frame.
        minimum: usize,
    },
    /// The length prefix does not match the frame length.
    #[error("the length prefix announces {announced} bytes, but the frame carries {actual}")]
    LengthMismatch {
        /// Body length announced by the prefix.
        announced: usize,
        /// Body length actually present.
        actual: usize,
    },
    /// The header length does not fit the body.
    #[error("a {header} byte header does not fit the {body} byte body")]
    HeaderDoesNotFit {
        /// Header length the caller named.
        header: usize,
        /// Body length available.
        body: usize,
    },
    /// The signature does not verify.
    #[error("the frame signature does not verify")]
    Signature,
    /// The payload could not be decrypted.
    #[error("the frame payload could not be decrypted")]
    Payload,
    /// The Ed25519 seed is invalid.
    #[error("the Ed25519 signing seed is invalid")]
    SigningKey,
    /// The recipient X25519 key is invalid.
    #[error("the recipient X25519 key is invalid")]
    RecipientKey,
    /// The encapsulated X25519 key is invalid.
    #[error("the encapsulated X25519 key is invalid")]
    EncapsulatedKey,
    /// The body length does not fit the four-byte prefix.
    #[error("the frame body is {0} bytes and does not fit the four-byte prefix")]
    BodyTooLarge(usize),
    /// HPKE failed to set up or complete the exchange.
    #[error(transparent)]
    Hpke(#[from] hpke::HpkeError),
}

/// Seal `header` and `payload` into a frame under `recipient_key`.
///
/// The result starts with the length prefix, which the signature also covers.
pub fn seal_frame(
    header: &[u8],
    payload: &[u8],
    recipient_key: &[u8; X25519_KEY_BYTES],
    signing_seed: &[u8; ED25519_KEY_BYTES],
) -> Result<Vec<u8>, FrameSealError> {
    let recipient = <X25519HkdfSha256 as hpke::Kem>::PublicKey::from_bytes(recipient_key)
        .map_err(|_| FrameSealError::RecipientKey)?;
    let (encapsulation, mut context) = hpke::setup_sender::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
        &OpModeS::Base,
        &recipient,
        b"",
    )?;
    let ciphertext = context.seal(payload, b"")?;
    let encapsulated = encapsulation.to_bytes();
    let body_length = header.len() + SIGNATURE_BYTES + encapsulated.len() + ciphertext.len();
    let announced =
        u32::try_from(body_length).map_err(|_| FrameSealError::BodyTooLarge(body_length))?;
    let prefix = announced.to_be_bytes();

    let mut signed = Vec::with_capacity(LENGTH_PREFIX_BYTES + body_length);
    signed.extend_from_slice(&prefix);
    signed.extend_from_slice(header);
    signed.extend_from_slice(encapsulated.as_slice());
    signed.extend_from_slice(&ciphertext);

    let key_pair = Ed25519KeyPair::from_seed_unchecked(signing_seed)
        .map_err(|_| FrameSealError::SigningKey)?;
    let signature = key_pair.sign(&signed);

    let mut frame = Vec::with_capacity(LENGTH_PREFIX_BYTES + body_length);
    frame.extend_from_slice(&prefix);
    frame.extend_from_slice(header);
    frame.extend_from_slice(signature.as_ref());
    frame.extend_from_slice(encapsulated.as_slice());
    frame.extend_from_slice(&ciphertext);

    Ok(frame)
}

/// Verify and decrypt a received frame, returning its payload.
///
/// `header_length` is the length of the encoded external header that the caller
/// decoded from the body. The signature covers the prefix, that header and the
/// payload, so the whole frame is authenticated before decryption.
pub fn open_frame(
    frame: &[u8],
    header_length: usize,
    recipient_secret: &[u8; X25519_KEY_BYTES],
    sender_public: &[u8; ED25519_KEY_BYTES],
) -> Result<Vec<u8>, FrameSealError> {
    let fixed = LENGTH_PREFIX_BYTES + SIGNATURE_BYTES + MINIMUM_PAYLOAD_BYTES;
    if frame.len() < fixed {
        return Err(FrameSealError::FrameTooShort {
            actual: frame.len(),
            minimum: fixed,
        });
    }
    let announced = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    let body = &frame[LENGTH_PREFIX_BYTES..];
    if announced != body.len() {
        return Err(FrameSealError::LengthMismatch {
            announced,
            actual: body.len(),
        });
    }
    if header_length
        > body
            .len()
            .saturating_sub(SIGNATURE_BYTES + MINIMUM_PAYLOAD_BYTES)
    {
        return Err(FrameSealError::HeaderDoesNotFit {
            header: header_length,
            body: body.len(),
        });
    }

    let payload_start = header_length + SIGNATURE_BYTES;
    let signature = &body[header_length..payload_start];
    let payload = &body[payload_start..];

    let mut signed = Vec::with_capacity(LENGTH_PREFIX_BYTES + header_length + payload.len());
    signed.extend_from_slice(&frame[..LENGTH_PREFIX_BYTES + header_length]);
    signed.extend_from_slice(payload);
    UnparsedPublicKey::new(&ED25519, sender_public)
        .verify(&signed, signature)
        .map_err(|_| FrameSealError::Signature)?;

    let (encapsulated_key, ciphertext) = payload.split_at(ENCAPSULATED_KEY_BYTES);
    let encapsulation = <X25519HkdfSha256 as hpke::Kem>::EncappedKey::from_bytes(encapsulated_key)
        .map_err(|_| FrameSealError::EncapsulatedKey)?;
    let recipient = <X25519HkdfSha256 as hpke::Kem>::PrivateKey::from_bytes(recipient_secret)
        .map_err(|_| FrameSealError::RecipientKey)?;
    let mut context = hpke::setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &recipient,
        &encapsulation,
        b"",
    )?;

    context
        .open(ciphertext, b"")
        .map_err(|_| FrameSealError::Payload)
}

#[cfg(test)]
mod tests {
    use ring::signature::KeyPair;

    use super::*;

    const HEADER: &[u8] = &[0x84, 0x01, 0x02, 0x03];
    const PAYLOAD: &[u8] = b"application payload";
    const SEED: [u8; ED25519_KEY_BYTES] = [0x11; ED25519_KEY_BYTES];

    fn array<const N: usize>(bytes: impl AsRef<[u8]>) -> [u8; N] {
        let bytes = bytes.as_ref();
        assert_eq!(bytes.len(), N);
        let mut out = [0_u8; N];
        out.copy_from_slice(bytes);
        out
    }

    fn recipient() -> ([u8; X25519_KEY_BYTES], [u8; X25519_KEY_BYTES]) {
        let (secret, public) = <X25519HkdfSha256 as hpke::Kem>::derive_keypair(&[0x42; 32]);
        (array(secret.to_bytes()), array(public.to_bytes()))
    }

    fn sender_public() -> [u8; ED25519_KEY_BYTES] {
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&SEED).expect("a valid seed");
        array(key_pair.public_key().as_ref())
    }

    fn open(frame: &[u8], secret: &[u8; X25519_KEY_BYTES]) -> Result<Vec<u8>, FrameSealError> {
        open_frame(frame, HEADER.len(), secret, &sender_public())
    }

    #[test]
    fn a_frame_round_trips() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");

        assert_eq!(open(&frame, &secret).expect("opening must work"), PAYLOAD);
        assert_eq!(
            &frame[LENGTH_PREFIX_BYTES..LENGTH_PREFIX_BYTES + HEADER.len()],
            HEADER
        );
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, b"", &public, &SEED).expect("sealing must work");

        assert_eq!(open(&frame, &secret).expect("opening must work"), b"");
    }

    #[test]
    fn every_frame_uses_a_fresh_encapsulation() {
        let (_, public) = recipient();
        let first = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let second = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let start = LENGTH_PREFIX_BYTES + HEADER.len() + SIGNATURE_BYTES;
        let end = start + ENCAPSULATED_KEY_BYTES;

        assert_ne!(&first[start..end], &second[start..end]);
    }

    #[test]
    fn the_signature_covers_the_prefix_the_header_and_the_payload() {
        let (_, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let signature_start = LENGTH_PREFIX_BYTES + HEADER.len();
        let payload_start = signature_start + SIGNATURE_BYTES;
        let mut signed = frame[..signature_start].to_vec();
        signed.extend_from_slice(&frame[payload_start..]);

        UnparsedPublicKey::new(&ED25519, sender_public())
            .verify(&signed, &frame[signature_start..payload_start])
            .expect("the signature must cover the prefix, header and payload");
    }

    #[test]
    fn a_tampered_header_does_not_verify() {
        let (secret, public) = recipient();
        let mut frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        frame[LENGTH_PREFIX_BYTES] ^= 0x01;

        assert!(matches!(
            open(&frame, &secret),
            Err(FrameSealError::Signature)
        ));
    }

    #[test]
    fn a_tampered_payload_does_not_verify() {
        let (secret, public) = recipient();
        let mut frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let last = frame.len() - 1;
        frame[last] ^= 0x01;

        assert!(matches!(
            open(&frame, &secret),
            Err(FrameSealError::Signature)
        ));
    }

    #[test]
    fn a_foreign_sender_does_not_verify() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let foreign = [0x99; ED25519_KEY_BYTES];

        assert!(matches!(
            open_frame(&frame, HEADER.len(), &secret, &foreign),
            Err(FrameSealError::Signature)
        ));
    }

    #[test]
    fn a_foreign_recipient_cannot_decrypt() {
        let (_, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");
        let foreign = [0x77; X25519_KEY_BYTES];

        assert!(matches!(
            open(&frame, &foreign),
            Err(FrameSealError::Payload)
        ));
    }

    #[test]
    fn a_wrong_declared_header_length_does_not_verify() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");

        assert!(matches!(
            open_frame(&frame, HEADER.len() - 1, &secret, &sender_public()),
            Err(FrameSealError::Signature)
        ));
        assert!(matches!(
            open_frame(&frame, HEADER.len() + 1, &secret, &sender_public()),
            Err(FrameSealError::Signature)
        ));
    }

    #[test]
    fn a_header_length_larger_than_the_body_is_rejected() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");

        assert!(matches!(
            open_frame(&frame, usize::MAX, &secret, &sender_public()),
            Err(FrameSealError::HeaderDoesNotFit { .. })
        ));
    }

    #[test]
    fn a_truncated_frame_is_rejected() {
        let (secret, public) = recipient();
        let frame = seal_frame(HEADER, PAYLOAD, &public, &SEED).expect("sealing must work");

        assert!(matches!(
            open(&frame[..frame.len() - 1], &secret),
            Err(FrameSealError::LengthMismatch { .. })
        ));
        assert!(matches!(
            open(&frame[..LENGTH_PREFIX_BYTES + 1], &secret),
            Err(FrameSealError::FrameTooShort { .. })
        ));
    }
}
