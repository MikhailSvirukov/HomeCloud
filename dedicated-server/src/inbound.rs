//! Inbound request path: from a stream to a decoded client logical header.
//!
//! One request is one frame of the connection (`INT-NET-002 / 1`,
//! `INT-NET-081 / 1`). The reader bounds the frame, locates the encoded
//! external header, verifies the signature with the peer key before any
//! decryption (`INT-NET-020 / 1`), applies the configured logical-header and
//! credential bounds (`INT-NET-085 / 1`, `INT-NET-086 / 1`) and decodes the
//! client logical header.

use std::io::Read;

use thiserror::Error;

use crate::config::ServerConfig;
use crate::frame::{read_frame, FrameError};
use crate::frame_seal::{open_frame, FrameSealError, X25519_KEY_BYTES};
use crate::wire::{ClientHeader, ExternalHeader, WireError};

/// Keys of the peer whose request is being read.
#[derive(Clone, Copy)]
pub struct PeerKeys {
    /// Ed25519 key that signed the request.
    pub signing_public_key: [u8; X25519_KEY_BYTES],
}

/// Errors produced while reading one inbound request.
#[derive(Debug, Error)]
pub enum InboundError {
    /// The frame could not be read within the configured limit.
    #[error("the incoming frame is invalid: {0}")]
    Frame(#[from] FrameError),
    /// The external header boundary could not be found.
    #[error("the external header boundary could not be found: {0}")]
    HeaderBoundary(String),
    /// The external header is invalid.
    #[error("the external header is invalid: {0}")]
    Header(#[from] WireError),
    /// The signature does not verify with the peer key.
    #[error("the request signature does not verify")]
    Signature,
    /// The payload could not be decrypted.
    #[error("the request payload could not be decrypted")]
    Payload,
    /// The decrypted logical header exceeds the configured limit.
    #[error("the logical header is {actual} bytes, above max_header_bytes ({max})")]
    LogicalHeaderTooLarge {
        /// Observed length in bytes.
        actual: usize,
        /// Configured maximum.
        max: usize,
    },
    /// The client logical header is invalid.
    #[error("the client logical header is invalid: {0}")]
    Client(WireError),
}

/// Read one client request from `stream` and return its decoded header.
///
/// `peer` carries the keys resolved by the caller before this call: the
/// bootstrap key for a first-contact frame, or the registered installation keys
/// for an established one. The frame is read within `max_frame_bytes`, the
/// signature is verified before any decryption, and the logical header and its
/// credentials are bounded before they are decoded.
pub fn read_request<R: Read>(
    stream: &mut R,
    config: &ServerConfig,
    server_secret: &[u8; X25519_KEY_BYTES],
    peer: PeerKeys,
) -> Result<(ExternalHeader, ClientHeader), InboundError> {
    let body = read_frame(stream, config.max_frame_bytes())?;
    let header_length = header_boundary(&body)?;
    let header = ExternalHeader::decode(&body[..header_length], config.max_label_bytes.get())?;

    let mut sealed = Vec::with_capacity(4 + body.len());
    sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    sealed.extend_from_slice(&body);
    let plaintext = open_frame(
        &sealed,
        header_length,
        server_secret,
        &peer.signing_public_key,
    )
    .map_err(|error| match error {
        FrameSealError::Signature => InboundError::Signature,
        _ => InboundError::Payload,
    })?;
    if plaintext.len() > config.max_header_bytes.get() {
        return Err(InboundError::LogicalHeaderTooLarge {
            actual: plaintext.len(),
            max: config.max_header_bytes.get(),
        });
    }

    let limits = config.header_limits();
    let client = ClientHeader::decode(&plaintext, &limits)?;

    Ok((header, client))
}

/// Length of the encoded external header at the start of `body`.
fn header_boundary(body: &[u8]) -> Result<usize, InboundError> {
    let mut cursor = body;
    let _header: ciborium::Value = ciborium::de::from_reader(&mut cursor)
        .map_err(|error| InboundError::HeaderBoundary(error.to_string()))?;

    Ok(body.len() - cursor.len())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use hpke::kem::X25519HkdfSha256;
    use hpke::Serializable;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;
    use crate::config::ServerConfig;
    use crate::frame_seal::seal_frame;
    use crate::wire::{ClientOperation, Credentials};

    const FIXTURE: &str = include_str!("../tests/fixtures/valid_server.toml");
    const SERVER_X25519_SEED: [u8; 32] = [0x31; 32];
    const CLIENT_ED25519_SEED: [u8; 32] = [0x61; 32];

    fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
        let mut out = [0_u8; N];
        out.copy_from_slice(&bytes[..N]);
        out
    }

    fn x25519(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
        let (secret, public) = <X25519HkdfSha256 as hpke::Kem>::derive_keypair(seed);
        (
            array(secret.to_bytes().as_slice()),
            array(public.to_bytes().as_slice()),
        )
    }

    fn ed25519(seed: &[u8; 32]) -> [u8; 32] {
        let pair = Ed25519KeyPair::from_seed_unchecked(seed).expect("a valid seed");
        array(pair.public_key().as_ref())
    }

    fn config() -> ServerConfig {
        let (_, server_x25519) = x25519(&SERVER_X25519_SEED);
        let document = FIXTURE
            .replace(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                &server_x25519
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            )
            .replace(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                &ed25519(&CLIENT_ED25519_SEED)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            );

        ServerConfig::from_toml_str(&document).expect("the test configuration must parse")
    }

    fn request_frame(header: &ExternalHeader, client: &ClientHeader) -> Vec<u8> {
        let header_bytes = header.encode(64).expect("the header must encode");
        let payload = client.encode().expect("the payload must encode");
        seal_frame(
            &header_bytes,
            &payload,
            &x25519(&SERVER_X25519_SEED).1,
            &CLIENT_ED25519_SEED,
        )
        .expect("the frame must seal")
    }

    fn peer() -> PeerKeys {
        PeerKeys {
            signing_public_key: ed25519(&CLIENT_ED25519_SEED),
        }
    }

    #[test]
    fn a_topology_request_is_read_and_decoded() {
        let config = config();
        let header = ExternalHeader {
            sender_id: Some([0x01; 16]),
            request_id: [0x11; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        };
        let client = ClientHeader::Topology {
            credentials: Credentials {
                login: "alice".to_owned(),
                password: "secret-password".to_owned(),
            },
        };
        // The frame prefix is part of the sealed body, so the reader is fed the
        // sealed frame directly through a cursor.
        let frame = request_frame(&header, &client);
        let mut cursor = Cursor::new(frame);

        let (read_header, read_client) =
            read_request(&mut cursor, &config, &x25519(&SERVER_X25519_SEED).0, peer())
                .expect("the request must be read");

        assert_eq!(read_header, header);
        assert_eq!(read_client, client);
        assert_eq!(read_client.operation(), ClientOperation::Topology);
    }

    #[test]
    fn a_foreign_signature_is_rejected() {
        let config = config();
        let header = ExternalHeader {
            sender_id: Some([0x01; 16]),
            request_id: [0x11; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        };
        let client = ClientHeader::Topology {
            credentials: Credentials {
                login: "alice".to_owned(),
                password: "secret-password".to_owned(),
            },
        };
        let header_bytes = header.encode(64).expect("the header must encode");
        let payload = client.encode().expect("the payload must encode");
        let frame = seal_frame(
            &header_bytes,
            &payload,
            &x25519(&SERVER_X25519_SEED).1,
            &[0x99; 32],
        )
        .expect("the frame must seal");
        let mut cursor = Cursor::new(frame);

        let error = read_request(&mut cursor, &config, &x25519(&SERVER_X25519_SEED).0, peer())
            .expect_err("a foreign signature must fail");

        assert!(matches!(error, InboundError::Signature));
    }

    #[test]
    fn a_logical_header_above_the_limit_is_rejected() {
        let mut config = config();
        config.max_header_bytes = std::num::NonZeroUsize::new(1_024).expect("a non-zero limit");
        let header = ExternalHeader {
            sender_id: Some([0x01; 16]),
            request_id: [0x11; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        };
        let client = ClientHeader::Upload {
            credentials: Credentials {
                login: "alice".to_owned(),
                password: "secret-password".to_owned(),
            },
            source_name: "x".repeat(2_000),
            timestamp_ms: 0,
            original_size_bytes: 1,
            compressed_size_bytes: 1,
            sha256: [0x33; 32],
        };
        let frame = request_frame(&header, &client);
        let mut cursor = Cursor::new(frame);

        let error = read_request(&mut cursor, &config, &x25519(&SERVER_X25519_SEED).0, peer())
            .expect_err("an oversized logical header must fail");

        assert!(matches!(
            error,
            InboundError::LogicalHeaderTooLarge { max: 1_024, .. }
        ));
    }
}
