//! Outbound inter-server exchange with a storage node.
//!
//! The dedicated server talks to a storage node from its static manifest over
//! one sealed frame per exchange (`INT-NET-089 / 1`, `INT-NET-090 / 1`): the
//! request carries the inter-server envelope `[message_type, args]` inside a
//! frame signed with the dedicated signing key and encrypted to the storage
//! node, and the response is verified with the manifest signing key before it
//! is decrypted (`INT-NET-020 / 1`). The external header of the response must
//! repeat the request identifier and come from the addressed node.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use ciborium::Value;
use thiserror::Error;

use crate::config::{ManifestEntry, ServerConfig};
use crate::frame::{read_frame, FrameError};
use crate::frame_seal::{open_frame, seal_frame, FrameSealError, X25519_KEY_BYTES};
use crate::identity::ServerIdentity;
use crate::wire::{
    ExternalHeader, InterServerMessage, RegistrySnapshot, WireError, IDENTIFIER_BYTES,
};

/// Result of one outbound exchange.
type Result<T> = core::result::Result<T, InterServerClientError>;

/// Errors produced while exchanging one inter-server message.
#[derive(Debug, Error)]
pub enum InterServerClientError {
    /// The static manifest names no storage node.
    #[error("the static manifest names no storage node")]
    NoStorageNode,
    /// A key file does not hold a 32-byte key.
    #[error("the {role} key is {actual} bytes, expected {expected}")]
    KeyLength {
        /// Key role that was read.
        role: &'static str,
        /// Observed length.
        actual: usize,
        /// Expected length.
        expected: usize,
    },
    /// A login is outside the configured byte range.
    #[error("login must be between {min} and {max} bytes, got {actual}")]
    LoginOutOfBounds {
        /// Observed length in bytes.
        actual: usize,
        /// Configured minimum.
        min: usize,
        /// Configured maximum.
        max: usize,
    },
    /// The decoded logical header exceeds the configured limit.
    #[error("the logical header is {actual} bytes, above max_header_bytes ({max})")]
    LogicalHeaderTooLarge {
        /// Observed length in bytes.
        actual: usize,
        /// Configured maximum.
        max: usize,
    },
    /// The connection attempt failed.
    #[error("connecting to {address} failed: {source}")]
    Connect {
        /// Address the client tried to reach.
        address: std::net::SocketAddr,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Writing the request or reading the response failed.
    #[error("the inter-server exchange failed: {0}")]
    Io(#[from] std::io::Error),
    /// A frame is malformed.
    #[error("the frame is invalid: {0}")]
    Frame(#[from] FrameError),
    /// A frame could not be sealed or opened.
    #[error("the frame could not be sealed or opened: {0}")]
    Seal(#[from] FrameSealError),
    /// An inter-server message is invalid.
    #[error("the inter-server message is invalid: {0}")]
    Wire(#[from] WireError),
    /// The encoded external header boundary could not be found.
    #[error("the response header boundary could not be found: {0}")]
    HeaderBoundary(String),
    /// The response does not belong to the request or the addressed node.
    #[error("the response does not match the request: {0}")]
    Mismatch(&'static str),
    /// The content tag could not be encoded.
    #[error("the registration content tag could not be computed: {0}")]
    Tag(String),
    /// The storage node answered with a non-zero result code.
    #[error("the storage node rejected the request with result code {0:#010x}")]
    Rejected(u32),
    /// The storage node answered with an unexpected message type.
    #[error("the storage node answered with an unexpected message type")]
    UnexpectedMessage,
}

/// Exchange one message with `storage` and return the response message.
fn exchange(
    config: &ServerConfig,
    identity: &ServerIdentity,
    storage: &ManifestEntry,
    request: &InterServerMessage,
    request_id: [u8; IDENTIFIER_BYTES],
) -> Result<InterServerMessage> {
    let signing_seed = key_seed(identity.signing_private_key(), "signing")?;
    let header = ExternalHeader {
        sender_id: Some(*config.dedicated.sender_id.as_bytes()),
        request_id,
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    };
    let header_bytes = header.encode(config.max_label_bytes.get())?;
    let payload = request.encode(config.max_verifier_bytes.get())?;
    let frame = seal_frame(
        &header_bytes,
        &payload,
        storage.encryption_public_key.as_bytes(),
        &signing_seed,
    )?;

    let connect_timeout = Duration::from_millis(config.connect_timeout_ms.get());
    let mut stream =
        TcpStream::connect_timeout(&storage.address, connect_timeout).map_err(|source| {
            InterServerClientError::Connect {
                address: storage.address,
                source,
            }
        })?;
    let exchange_timeout = Duration::from_millis(config.inter_server_timeout_ms.get());
    stream.set_read_timeout(Some(exchange_timeout))?;
    stream.set_write_timeout(Some(exchange_timeout))?;
    stream.write_all(&frame)?;

    let body = read_frame(&mut stream, config.max_frame_bytes())?;
    let header_length = external_header_length(&body)?;
    let response_header =
        ExternalHeader::decode(&body[..header_length], config.max_label_bytes.get())?;
    if response_header.sender_id != Some(*storage.sender_id.as_bytes()) {
        return Err(InterServerClientError::Mismatch(
            "the response sender is not the addressed node",
        ));
    }
    if response_header.request_id != request_id {
        return Err(InterServerClientError::Mismatch(
            "the response does not repeat the request identifier",
        ));
    }
    if response_header.frame_number != 0 {
        return Err(InterServerClientError::Mismatch(
            "the response does not carry the first frame",
        ));
    }
    if response_header.responder_label.as_deref() != Some(storage.label.as_str()) {
        return Err(InterServerClientError::Mismatch(
            "the response label is not the addressed node label",
        ));
    }
    if response_header.upload_id.is_some() {
        return Err(InterServerClientError::Mismatch(
            "a registry result does not carry an upload identifier",
        ));
    }

    let mut sealed = Vec::with_capacity(4 + body.len());
    sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    sealed.extend_from_slice(&body);
    let recipient_secret = key_seed(identity.encryption_private_key(), "encryption")?;
    let plaintext = open_frame(
        &sealed,
        header_length,
        &recipient_secret,
        storage.signing_public_key.as_bytes(),
    )?;
    ensure_header_bound(&plaintext, config.max_header_bytes.get())?;

    InterServerMessage::decode(&plaintext, config.max_verifier_bytes.get())
        .map_err(InterServerClientError::Wire)
}

/// Reject a decoded logical header above the configured limit (`INT-NET-085`).
fn ensure_header_bound(plaintext: &[u8], max_header_bytes: usize) -> Result<()> {
    if plaintext.len() > max_header_bytes {
        return Err(InterServerClientError::LogicalHeaderTooLarge {
            actual: plaintext.len(),
            max: max_header_bytes,
        });
    }

    Ok(())
}

/// Reject a login outside the configured byte range (`CFG-CL-029`).
fn ensure_login_bounds(login: &str, min: usize, max: usize) -> Result<()> {
    if login.len() < min || login.len() > max {
        return Err(InterServerClientError::LoginOutOfBounds {
            actual: login.len(),
            min,
            max,
        });
    }

    Ok(())
}

/// Read the 32-byte key out of `bytes`.
fn key_seed(bytes: &[u8], role: &'static str) -> Result<[u8; X25519_KEY_BYTES]> {
    let seed: [u8; X25519_KEY_BYTES] =
        bytes
            .try_into()
            .map_err(|_| InterServerClientError::KeyLength {
                role,
                actual: bytes.len(),
                expected: X25519_KEY_BYTES,
            })?;

    Ok(seed)
}

/// A registration command committed by a storage node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationCommit {
    /// Installation identifier committed by the command.
    pub installation_id: [u8; IDENTIFIER_BYTES],
    /// Raft index of the commit.
    pub commit_index: u64,
}

/// Inputs of one registration propose command.
pub struct RegistrationProposeRequest<'a> {
    /// Login being registered.
    pub login: &'a str,
    /// Password of the registration.
    pub password: &'a str,
    /// Installation identifier assigned by the dedicated server.
    pub installation_id: [u8; IDENTIFIER_BYTES],
    /// Logical registration request identifier.
    pub request_id: [u8; IDENTIFIER_BYTES],
    /// Open X25519 key of the installation.
    pub x25519_key: [u8; 32],
    /// Open Ed25519 key of the installation.
    pub ed25519_key: [u8; 32],
    /// Current verifier of an existing account the command must match.
    pub expected_verifier: Option<&'a str>,
    /// New verifier of an account created by the command.
    pub new_verifier: Option<&'a str>,
}

/// Propose one registration command and return its committed result.
///
/// The content tag is computed from the secret, the credentials and the
/// installation keys (`INT-NET-109 / 1`); the response carries the installation
/// identifier and the commit index (`INT-NET-092 / 1`).
pub(crate) fn propose_registration(
    config: &ServerConfig,
    identity: &ServerIdentity,
    secret: &crate::registration_tag::RegistrationSecret,
    request: &RegistrationProposeRequest<'_>,
) -> Result<RegistrationCommit> {
    let storage = config
        .storage
        .first()
        .ok_or(InterServerClientError::NoStorageNode)?;
    ensure_login_bounds(
        request.login,
        config.min_login_bytes.get(),
        config.max_login_bytes.get(),
    )?;
    let content_tag = crate::registration_tag::content_tag(
        secret,
        request.login,
        request.password,
        &request.x25519_key,
        &request.ed25519_key,
    )
    .map_err(|error| InterServerClientError::Tag(error.to_string()))?;
    let message = InterServerMessage::RegistrationPropose {
        login: request.login.to_owned(),
        registration_request_id: request.request_id,
        content_tag,
        installation_id: request.installation_id,
        x25519_key: request.x25519_key,
        ed25519_key: request.ed25519_key,
        expected_verifier: request.expected_verifier.map(str::to_owned),
        new_verifier: request.new_verifier.map(str::to_owned),
    };
    let exchange_id: [u8; IDENTIFIER_BYTES] = *uuid::Uuid::new_v4().as_bytes();

    match exchange(config, identity, storage, &message, exchange_id)? {
        InterServerMessage::RegistrationResult {
            code: 0,
            installation_id: Some(installation_id),
            commit_index: Some(commit_index),
        } => Ok(RegistrationCommit {
            installation_id,
            commit_index,
        }),
        InterServerMessage::RegistrationResult { code, .. } => {
            Err(InterServerClientError::Rejected(code))
        }
        _ => Err(InterServerClientError::UnexpectedMessage),
    }
}

/// Length of the encoded external header at the start of `body`.
///
/// ciborium consumes exactly one encoded item from the slice, so the remaining
/// suffix length yields the header length.
fn external_header_length(body: &[u8]) -> Result<usize> {
    let mut cursor = body;
    let _header: Value = ciborium::de::from_reader(&mut cursor)
        .map_err(|error| InterServerClientError::HeaderBoundary(error.to_string()))?;

    Ok(body.len() - cursor.len())
}

/// Look up the registry of a storage node.
///
/// The first storage entry of the static manifest is used; failover across
/// storage nodes is a separate step (`FR-DED-046`).
pub(crate) fn lookup_registry(
    config: &ServerConfig,
    identity: &ServerIdentity,
    login: Option<&str>,
    registration_request_id: Option<[u8; IDENTIFIER_BYTES]>,
    installation_id: Option<[u8; IDENTIFIER_BYTES]>,
) -> Result<RegistrySnapshot> {
    let storage = config
        .storage
        .first()
        .ok_or(InterServerClientError::NoStorageNode)?;
    if let Some(login) = login {
        ensure_login_bounds(
            login,
            config.min_login_bytes.get(),
            config.max_login_bytes.get(),
        )?;
    }
    let request_id: [u8; IDENTIFIER_BYTES] = *uuid::Uuid::new_v4().as_bytes();
    let request = InterServerMessage::RegistryLookup {
        login: login.map(str::to_owned),
        registration_request_id,
        installation_id,
    };

    match exchange(config, identity, storage, &request, request_id)? {
        InterServerMessage::RegistryLookupResult { code: 0, snapshot } => {
            let snapshot = snapshot.ok_or(InterServerClientError::UnexpectedMessage)?;
            if let Some(installation) = &snapshot.installation {
                ensure_login_bounds(
                    &installation.login,
                    config.min_login_bytes.get(),
                    config.max_login_bytes.get(),
                )?;
            }
            Ok(snapshot)
        }
        InterServerMessage::RegistryLookupResult { code, .. } => {
            Err(InterServerClientError::Rejected(code))
        }
        _ => Err(InterServerClientError::UnexpectedMessage),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;

    use hpke::kem::X25519HkdfSha256;
    use hpke::Serializable;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;
    use crate::config::ServerConfig;
    use crate::registration_tag::{content_tag, RegistrationSecret};

    const FIXTURE: &str = include_str!("../tests/fixtures/valid_server.toml");
    const DEDICATED_ID: [u8; 16] = [0x00; 16];
    const STORAGE_ID: [u8; 16] = {
        let mut id = [0x00; 16];
        id[15] = 0x01;
        id
    };
    const DEDICATED_X25519_SEED: [u8; 32] = [0x31; 32];
    const DEDICATED_ED25519_SEED: [u8; 32] = [0x41; 32];
    const STORAGE_X25519_SEED: [u8; 32] = [0x51; 32];
    const STORAGE_ED25519_SEED: [u8; 32] = [0x61; 32];
    const FOREIGN_ED25519_SEED: [u8; 32] = [0x71; 32];

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn x25519(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
        let (secret, public) = <X25519HkdfSha256 as hpke::Kem>::derive_keypair(seed);
        let mut secret_bytes = [0_u8; 32];
        secret_bytes.copy_from_slice(secret.to_bytes().as_slice());
        let mut public_bytes = [0_u8; 32];
        public_bytes.copy_from_slice(public.to_bytes().as_slice());

        (secret_bytes, public_bytes)
    }

    fn ed25519(seed: &[u8; 32]) -> [u8; 32] {
        let pair = Ed25519KeyPair::from_seed_unchecked(seed).expect("a valid seed");
        let mut public = [0_u8; 32];
        public.copy_from_slice(pair.public_key().as_ref());

        public
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT_ID: AtomicU64 = AtomicU64::new(0);

            let path = std::env::temp_dir().join(format!(
                "homecloud-lookup-{name}-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the test directory must be created");
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn identity(dir: &TempDir) -> ServerIdentity {
        let encryption = dir.file("encryption.key");
        let signing = dir.file("signing.key");
        let (dedicated_x25519, _) = x25519(&DEDICATED_X25519_SEED);
        std::fs::write(&encryption, dedicated_x25519).expect("the encryption key must exist");
        std::fs::write(&signing, DEDICATED_ED25519_SEED).expect("the signing key must exist");
        ServerIdentity::load("dedicated-1", &encryption, &signing).expect("the identity must load")
    }

    fn config_with(port: u16, header_limit: Option<usize>) -> ServerConfig {
        let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
        let dedicated_ed25519 = ed25519(&DEDICATED_ED25519_SEED);
        let (_, storage_x25519) = x25519(&STORAGE_X25519_SEED);
        let storage_ed25519 = ed25519(&STORAGE_ED25519_SEED);
        let mut document = FIXTURE
            .replace(
                "address = \"10.0.0.11:7100\"",
                &format!("address = \"127.0.0.1:{port}\""),
            )
            .replace(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                &hex(&dedicated_x25519),
            )
            .replace(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                &hex(&dedicated_ed25519),
            )
            .replace(
                "1111111111111111111111111111111111111111111111111111111111111111",
                &hex(&storage_x25519),
            )
            .replace(
                "2222222222222222222222222222222222222222222222222222222222222222",
                &hex(&storage_ed25519),
            );
        if let Some(limit) = header_limit {
            document = document
                .replace(
                    "max_header_bytes = 1048576",
                    &format!("max_header_bytes = {limit}"),
                )
                .replace("max_password_bytes = 1024", "max_password_bytes = 8");
        }

        ServerConfig::from_toml_str(&document).expect("the test configuration must parse")
    }

    fn config(port: u16) -> ServerConfig {
        config_with(port, None)
    }

    struct MockResponse {
        sender_id: Option<[u8; 16]>,
        request_id: Option<[u8; 16]>,
        frame_number: u32,
        upload_id: Option<[u8; 16]>,
        responder_label: Option<String>,
        code: u32,
        snapshot: Option<RegistrySnapshot>,
    }

    impl MockResponse {
        fn empty() -> Self {
            Self {
                sender_id: Some(STORAGE_ID),
                request_id: None,
                frame_number: 0,
                upload_id: None,
                responder_label: Some("storage-1".to_owned()),
                code: 0,
                snapshot: Some(RegistrySnapshot {
                    read_index: 1,
                    account: None,
                    registration: None,
                    installation: None,
                }),
            }
        }

        fn snapshot(snapshot: RegistrySnapshot) -> Self {
            Self {
                snapshot: Some(snapshot),
                ..Self::empty()
            }
        }
    }

    /// What the mock storage node sends back.
    enum Reply {
        /// A well-formed response with its own header fields.
        Message(MockResponse),
        /// A response sealed with a key that is not the manifest signing key.
        ForeignSignature(MockResponse),
        /// A frame whose external header is not valid CBOR.
        MalformedHeader,
    }

    fn mock_storage(listener: TcpListener, reply: Reply) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client must connect");
            let limit = std::num::NonZeroU32::new(4 * 1024 * 1024).expect("a non-zero limit");
            let body = read_frame(&mut stream, limit).expect("the request must arrive");
            let header_length =
                external_header_length(&body).expect("the request header must be found");
            let request_header =
                ExternalHeader::decode(&body[..header_length], 64).expect("the header must parse");
            assert_eq!(request_header.sender_id, Some(DEDICATED_ID));
            assert_eq!(request_header.frame_number, 0);
            assert_eq!(request_header.upload_id, None);
            assert_eq!(request_header.responder_label, None);
            assert_eq!(request_header.bootstrap_signing_public_key, None);
            let mut sealed = Vec::with_capacity(4 + body.len());
            sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
            sealed.extend_from_slice(&body);
            let (storage_x25519, _) = x25519(&STORAGE_X25519_SEED);
            let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
            let plaintext = open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
                .expect("the request must open");
            let request =
                InterServerMessage::decode(&plaintext, 512).expect("the request must parse");
            match request {
                InterServerMessage::RegistryLookup { login, .. } => {
                    assert_eq!(login.as_deref(), Some("alice"));
                }
                _ => panic!("the request must be a registry lookup"),
            }

            let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
            let (response, signing_seed) = match reply {
                Reply::Message(response) => (response, STORAGE_ED25519_SEED),
                Reply::ForeignSignature(response) => (response, FOREIGN_ED25519_SEED),
                Reply::MalformedHeader => {
                    let frame = seal_frame(
                        &[0xFF; 4],
                        &[0x80],
                        &dedicated_x25519,
                        &STORAGE_ED25519_SEED,
                    )
                    .expect("the malformed response must seal");
                    stream
                        .write_all(&frame)
                        .expect("the malformed response must be written");
                    return;
                }
            };
            let response_message = InterServerMessage::RegistryLookupResult {
                code: response.code,
                snapshot: response.snapshot,
            };
            let payload = response_message
                .encode(512)
                .expect("the response must encode");
            let header = ExternalHeader {
                sender_id: response.sender_id,
                request_id: response.request_id.unwrap_or(request_header.request_id),
                frame_number: response.frame_number,
                upload_id: response.upload_id,
                responder_label: response.responder_label,
                bootstrap_signing_public_key: None,
            };
            let header_bytes = header.encode(64).expect("the response header must encode");
            let frame = seal_frame(&header_bytes, &payload, &dedicated_x25519, &signing_seed)
                .expect("the response must seal");
            stream
                .write_all(&frame)
                .expect("the response must be written");
        })
    }

    fn secret(dir: &TempDir) -> RegistrationSecret {
        let path = dir.file("registration.secret");
        std::fs::write(&path, [0x24_u8; 32]).expect("the secret must be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the secret permissions must be set");
        }
        RegistrationSecret::load(&path).expect("the secret must load")
    }

    fn mock_storage_propose(
        listener: TcpListener,
        expected_tag: [u8; 32],
        installation_id: [u8; 16],
        code: u32,
        commit_index: u64,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client must connect");
            let limit = std::num::NonZeroU32::new(4 * 1024 * 1024).expect("a non-zero limit");
            let body = read_frame(&mut stream, limit).expect("the request must arrive");
            let header_length =
                external_header_length(&body).expect("the request header must be found");
            let request_header =
                ExternalHeader::decode(&body[..header_length], 64).expect("the header must parse");
            let mut sealed = Vec::with_capacity(4 + body.len());
            sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
            sealed.extend_from_slice(&body);
            let (storage_x25519, _) = x25519(&STORAGE_X25519_SEED);
            let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
            let plaintext = open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
                .expect("the request must open");
            let request =
                InterServerMessage::decode(&plaintext, 512).expect("the request must parse");
            match request {
                InterServerMessage::RegistrationPropose {
                    login,
                    content_tag,
                    installation_id: sent,
                    x25519_key,
                    ed25519_key,
                    expected_verifier,
                    new_verifier,
                    ..
                } => {
                    assert_eq!(login, "alice");
                    assert_eq!(content_tag, expected_tag);
                    assert_eq!(sent, installation_id);
                    assert_eq!(x25519_key, [0x44; 32]);
                    assert_eq!(ed25519_key, [0x55; 32]);
                    assert_eq!(expected_verifier, None);
                    assert_eq!(new_verifier.as_deref(), Some("new-phc"));
                }
                _ => panic!("the request must be a registration propose"),
            }

            let response = InterServerMessage::RegistrationResult {
                code,
                installation_id: (code == 0).then_some(installation_id),
                commit_index: (code == 0).then_some(commit_index),
            };
            let payload = response.encode(512).expect("the response must encode");
            let header = ExternalHeader {
                sender_id: Some(STORAGE_ID),
                request_id: request_header.request_id,
                frame_number: 0,
                upload_id: None,
                responder_label: Some("storage-1".to_owned()),
                bootstrap_signing_public_key: None,
            };
            let header_bytes = header.encode(64).expect("the response header must encode");
            let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
            let frame = seal_frame(
                &header_bytes,
                &payload,
                &dedicated_x25519,
                &STORAGE_ED25519_SEED,
            )
            .expect("the response must seal");
            stream
                .write_all(&frame)
                .expect("the response must be written");
        })
    }

    fn propose_with(
        expected_tag: [u8; 32],
        installation_id: [u8; 16],
        code: u32,
        commit_index: u64,
    ) -> Result<RegistrationCommit> {
        let dir = TempDir::new("propose");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config(port);
        let server =
            mock_storage_propose(listener, expected_tag, installation_id, code, commit_index);
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id,
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };
        let result = propose_registration(&config, &identity, &secret, &request);
        server.join().expect("the mock server must finish");

        result
    }

    fn lookup_reply(reply: Reply) -> Result<RegistrySnapshot> {
        lookup_reply_with(reply, None)
    }

    fn lookup_with(response: MockResponse) -> Result<RegistrySnapshot> {
        lookup_reply(Reply::Message(response))
    }

    fn lookup_reply_with(reply: Reply, header_limit: Option<usize>) -> Result<RegistrySnapshot> {
        let dir = TempDir::new("lookup");
        let identity = identity(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with(port, header_limit);
        let server = mock_storage(listener, reply);
        let result = lookup_registry(&config, &identity, Some("alice"), None, None);
        server.join().expect("the mock server must finish");

        result
    }

    fn installation(login: &str) -> crate::wire::InstallationRecord {
        crate::wire::InstallationRecord {
            login: login.to_owned(),
            installation_id: [0x33; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
        }
    }

    #[test]
    fn a_propose_returns_the_committed_installation() {
        let dir = TempDir::new("tag");
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let installation_id = [0x66; 16];

        let commit = propose_with(expected_tag, installation_id, 0, 9)
            .expect("the propose must be committed");

        assert_eq!(
            commit,
            RegistrationCommit {
                installation_id,
                commit_index: 9
            }
        );
    }

    #[test]
    fn a_rejected_propose_reports_the_storage_result_code() {
        let dir = TempDir::new("tag-rejected");
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");

        let error = propose_with(expected_tag, [0x66; 16], 0x0003_0001, 0)
            .expect_err("a rejected propose must fail");

        assert!(matches!(
            error,
            InterServerClientError::Rejected(0x0003_0001)
        ));
    }

    #[test]
    fn a_lookup_returns_the_storage_snapshot() {
        let snapshot = RegistrySnapshot {
            read_index: 7,
            account: None,
            registration: None,
            installation: Some(installation("alice")),
        };

        let found =
            lookup_with(MockResponse::snapshot(snapshot.clone())).expect("the lookup must succeed");

        assert_eq!(found, snapshot);
    }

    #[test]
    fn a_rejected_lookup_reports_the_storage_result_code() {
        let response = MockResponse {
            code: 0x0001_0002,
            snapshot: None,
            ..MockResponse::empty()
        };

        let error = lookup_with(response).expect_err("a rejected lookup must fail");

        assert!(matches!(
            error,
            InterServerClientError::Rejected(0x0001_0002)
        ));
    }

    #[test]
    fn a_response_from_another_sender_is_rejected() {
        let response = MockResponse {
            sender_id: Some([0x99; 16]),
            ..MockResponse::empty()
        };

        assert!(matches!(
            lookup_with(response),
            Err(InterServerClientError::Mismatch(_))
        ));
    }

    #[test]
    fn a_response_with_another_request_identifier_is_rejected() {
        let response = MockResponse {
            request_id: Some([0x77; 16]),
            ..MockResponse::empty()
        };

        assert!(matches!(
            lookup_with(response),
            Err(InterServerClientError::Mismatch(_))
        ));
    }

    #[test]
    fn a_response_with_another_frame_number_is_rejected() {
        let response = MockResponse {
            frame_number: 1,
            ..MockResponse::empty()
        };

        assert!(matches!(
            lookup_with(response),
            Err(InterServerClientError::Mismatch(_))
        ));
    }

    #[test]
    fn a_response_with_another_label_is_rejected() {
        let response = MockResponse {
            responder_label: Some("storage-2".to_owned()),
            ..MockResponse::empty()
        };

        assert!(matches!(
            lookup_with(response),
            Err(InterServerClientError::Mismatch(_))
        ));
    }

    #[test]
    fn a_response_with_an_upload_identifier_is_rejected() {
        let response = MockResponse {
            upload_id: Some([0x55; 16]),
            ..MockResponse::empty()
        };

        assert!(matches!(
            lookup_with(response),
            Err(InterServerClientError::Mismatch(_))
        ));
    }

    #[test]
    fn a_response_signed_by_another_key_is_rejected() {
        assert!(matches!(
            lookup_reply(Reply::ForeignSignature(MockResponse::empty())),
            Err(InterServerClientError::Seal(FrameSealError::Signature))
        ));
    }

    #[test]
    fn a_response_with_a_malformed_header_is_rejected() {
        assert!(matches!(
            lookup_reply(Reply::MalformedHeader),
            Err(InterServerClientError::HeaderBoundary(_))
        ));
    }

    #[test]
    fn an_oversized_reply_header_is_rejected_before_decoding() {
        let snapshot = RegistrySnapshot {
            read_index: 1,
            account: None,
            registration: None,
            installation: Some(installation(&"a".repeat(2_000))),
        };

        assert!(matches!(
            lookup_reply_with(
                Reply::Message(MockResponse::snapshot(snapshot)),
                Some(1_024)
            ),
            Err(InterServerClientError::LogicalHeaderTooLarge { max: 1_024, .. })
        ));
    }

    #[test]
    fn a_returned_login_outside_the_configured_range_is_rejected() {
        let snapshot = RegistrySnapshot {
            read_index: 1,
            account: None,
            registration: None,
            installation: Some(installation(&"a".repeat(65))),
        };

        assert!(matches!(
            lookup_with(MockResponse::snapshot(snapshot)),
            Err(InterServerClientError::LoginOutOfBounds {
                actual: 65,
                max: 64,
                ..
            })
        ));
    }

    #[test]
    fn a_login_outside_the_configured_range_is_rejected_before_connecting() {
        let dir = TempDir::new("login");
        let identity = identity(&dir);
        let config = config(9);
        let login = "a".repeat(65);

        assert!(matches!(
            lookup_registry(&config, &identity, Some(&login), None, None),
            Err(InterServerClientError::LoginOutOfBounds {
                actual: 65,
                min: 1,
                max: 64
            })
        ));
    }

    #[test]
    fn the_header_bound_rejects_an_oversized_plaintext() {
        assert!(matches!(
            ensure_header_bound(&[0x00; 129], 128),
            Err(InterServerClientError::LogicalHeaderTooLarge {
                actual: 129,
                max: 128
            })
        ));
        assert!(ensure_header_bound(&[0x00; 128], 128).is_ok());
    }

    #[test]
    fn the_header_boundary_is_the_encoded_header_only() {
        let header = ExternalHeader {
            sender_id: Some([0x11; 16]),
            request_id: [0x11; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        };
        let mut body = header.encode(64).expect("the header must encode");
        let length = body.len();
        body.extend_from_slice(&[0xAA; 96]);

        assert_eq!(
            external_header_length(&body).expect("the boundary must be found"),
            length
        );
    }
}
