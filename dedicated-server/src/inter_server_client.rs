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
use std::time::{Duration, Instant};

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
    /// The total registration deadline passed.
    #[error("the registration deadline of {millis} ms passed before an exchange succeeded")]
    Deadline {
        /// Configured total deadline in milliseconds.
        millis: u64,
    },
    /// The content tag could not be computed.
    #[error("the registration content tag could not be computed: {0}")]
    Tag(#[from] crate::registration_tag::RegistrationSecretError),
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
    attempt_budget: Duration,
) -> Result<InterServerMessage> {
    let started = Instant::now();
    let request_id: [u8; IDENTIFIER_BYTES] = *uuid::Uuid::new_v4().as_bytes();
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

    let connect_timeout =
        Duration::from_millis(config.connect_timeout_ms.get()).min(attempt_budget);
    let mut stream =
        TcpStream::connect_timeout(&storage.address, connect_timeout).map_err(|source| {
            InterServerClientError::Connect {
                address: storage.address,
                source,
            }
        })?;
    let remaining = attempt_budget.saturating_sub(started.elapsed());
    let exchange_timeout =
        Duration::from_millis(config.inter_server_timeout_ms.get()).min(remaining);
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
    let deadline =
        Instant::now() + Duration::from_millis(config.registration.total_deadline_ms.get());
    propose_registration_until(config, identity, secret, request, deadline)
}

/// Propose one registration command within an existing registration deadline.
pub(crate) fn propose_registration_until(
    config: &ServerConfig,
    identity: &ServerIdentity,
    secret: &crate::registration_tag::RegistrationSecret,
    request: &RegistrationProposeRequest<'_>,
    deadline: Instant,
) -> Result<RegistrationCommit> {
    if config.storage.is_empty() {
        return Err(InterServerClientError::NoStorageNode);
    }
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
    )?;
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
    let response = with_registration_budget(config, deadline, |storage, budget| {
        let response = exchange(config, identity, storage, &message, budget)?;
        match &response {
            InterServerMessage::RegistrationResult { code, .. } if is_temporary_code(*code) => {
                Err(InterServerClientError::Rejected(*code))
            }
            _ => Ok(response),
        }
    })?;

    match response {
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

/// Run one attempt per configured attempt, rotating through the storage nodes.
///
/// Each attempt is bounded by the per-attempt deadline, the pause between
/// attempts comes from the configuration, and the loop stops at the attempt
/// count or the total registration deadline (`CFG-CL-042`-`CFG-CL-045`).
fn with_registration_budget<T>(
    config: &ServerConfig,
    deadline: Instant,
    mut attempt: impl FnMut(&ManifestEntry, Duration) -> Result<T>,
) -> Result<T> {
    let attempts = config.registration.attempts.get();
    let retry = Duration::from_millis(config.registration.retry_interval_ms.get());
    let configured_attempt = Duration::from_millis(config.registration.attempt_deadline_ms.get());
    let total = Duration::from_millis(config.registration.total_deadline_ms.get());
    let mut last = InterServerClientError::NoStorageNode;

    for index in 0..attempts {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let attempt_budget = configured_attempt.min(remaining);
        if attempt_budget.is_zero() {
            if matches!(last, InterServerClientError::Rejected(code) if is_temporary_code(code)) {
                return Err(last);
            }
            return Err(InterServerClientError::Deadline {
                millis: total.as_millis() as u64,
            });
        }
        let storage = &config.storage[(index as usize) % config.storage.len()];
        match attempt(storage, attempt_budget) {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) => last = error,
            Err(error) => return Err(error),
        }
        if index + 1 < attempts {
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(retry.min(remaining));
        }
    }

    Err(last)
}

/// Whether a failed exchange may succeed against another attempt.
///
/// Only a transport failure is retried; a protocol rejection, a malformed
/// frame or an invalid signature is final (`INT-NET-065 / 1`).
fn is_transient(error: &InterServerClientError) -> bool {
    match error {
        InterServerClientError::Connect { .. } | InterServerClientError::Io(_) => true,
        InterServerClientError::Frame(FrameError::Io(_)) => true,
        InterServerClientError::Rejected(code) => is_temporary_code(*code),
        _ => false,
    }
}

/// Whether a storage result code asks for a later attempt (`INT-NET-066 / 1`).
fn is_temporary_code(code: u32) -> bool {
    code == crate::result_code::ResultCode::QuorumUnavailable.as_u32()
        || code == crate::result_code::ResultCode::TemporaryUnavailable.as_u32()
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
/// The attempt count, the per-attempt deadline, the retry pause and the total
/// deadline come from the configuration, and the attempts rotate through the
/// storage entries of the static manifest in order (`FR-DED-046`-`FR-DED-053`).
pub(crate) fn lookup_registry(
    config: &ServerConfig,
    identity: &ServerIdentity,
    login: Option<&str>,
    registration_request_id: Option<[u8; IDENTIFIER_BYTES]>,
    installation_id: Option<[u8; IDENTIFIER_BYTES]>,
) -> Result<RegistrySnapshot> {
    let deadline =
        Instant::now() + Duration::from_millis(config.registration.total_deadline_ms.get());
    lookup_registry_until(
        config,
        identity,
        login,
        registration_request_id,
        installation_id,
        deadline,
    )
}

/// Look the registry up within an existing registration deadline.
pub(crate) fn lookup_registry_until(
    config: &ServerConfig,
    identity: &ServerIdentity,
    login: Option<&str>,
    registration_request_id: Option<[u8; IDENTIFIER_BYTES]>,
    installation_id: Option<[u8; IDENTIFIER_BYTES]>,
    deadline: Instant,
) -> Result<RegistrySnapshot> {
    if config.storage.is_empty() {
        return Err(InterServerClientError::NoStorageNode);
    }
    if let Some(login) = login {
        ensure_login_bounds(
            login,
            config.min_login_bytes.get(),
            config.max_login_bytes.get(),
        )?;
    }
    let request = InterServerMessage::RegistryLookup {
        login: login.map(str::to_owned),
        registration_request_id,
        installation_id,
    };

    let response = with_registration_budget(config, deadline, |storage, budget| {
        let message = exchange(config, identity, storage, &request, budget)?;
        match &message {
            InterServerMessage::RegistryLookupResult { code, .. } if is_temporary_code(*code) => {
                Err(InterServerClientError::Rejected(*code))
            }
            _ => Ok(message),
        }
    })?;

    match response {
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
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
    const STORAGE2_X25519_SEED: [u8; 32] = [0x53; 32];
    const STORAGE2_ED25519_SEED: [u8; 32] = [0x63; 32];
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

    /// Two storage entries: an unreachable first node and the mock on the second.
    /// Reserve and release a loopback port so a connection to it is refused.
    fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        listener
            .local_addr()
            .expect("the listener must have an address")
            .port()
    }

    fn config_with_two_nodes(first_port: u16, second_port: u16, attempts: u32) -> ServerConfig {
        let document = FIXTURE
            .replace(
                "address = \"10.0.0.11:7100\"",
                &format!("address = \"127.0.0.1:{first_port}\""),
            )
            .replace(
                "address = \"10.0.0.12:7100\"",
                &format!("address = \"127.0.0.1:{second_port}\""),
            )
            .replace("attempt_deadline_ms = 10000", "attempt_deadline_ms = 1000")
            .replace("attempts = 5", &format!("attempts = {attempts}"))
            .replace("retry_interval_ms = 1000", "retry_interval_ms = 1")
            .replace("total_deadline_ms = 30000", "total_deadline_ms = 5000")
            .replace(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                &hex(&x25519(&DEDICATED_X25519_SEED).1),
            )
            .replace(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                &hex(&ed25519(&DEDICATED_ED25519_SEED)),
            )
            .replace(
                "1111111111111111111111111111111111111111111111111111111111111111",
                &hex(&x25519(&STORAGE_X25519_SEED).1),
            )
            .replace(
                "2222222222222222222222222222222222222222222222222222222222222222",
                &hex(&ed25519(&STORAGE_ED25519_SEED)),
            )
            .replace(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &hex(&x25519(&STORAGE2_X25519_SEED).1),
            )
            .replace(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &hex(&ed25519(&STORAGE2_ED25519_SEED)),
            );

        ServerConfig::from_toml_str(&document).expect("the two-node configuration must parse")
    }

    fn config_with_failover(
        first_port: u16,
        mock_port: u16,
        attempts: u32,
        retry_ms: u64,
        total_ms: u64,
    ) -> ServerConfig {
        let document = FIXTURE
            .replace(
                "address = \"10.0.0.11:7100\"",
                &format!("address = \"127.0.0.1:{first_port}\""),
            )
            .replace(
                "address = \"10.0.0.12:7100\"",
                &format!("address = \"127.0.0.1:{mock_port}\""),
            )
            .replace(
                "attempt_deadline_ms = 10000",
                &format!("attempt_deadline_ms = {total_ms}"),
            )
            .replace("attempts = 5", &format!("attempts = {attempts}"))
            .replace(
                "retry_interval_ms = 1000",
                &format!("retry_interval_ms = {retry_ms}"),
            )
            .replace(
                "total_deadline_ms = 30000",
                &format!("total_deadline_ms = {total_ms}"),
            )
            .replace(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                &hex(&x25519(&DEDICATED_X25519_SEED).1),
            )
            .replace(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                &hex(&ed25519(&DEDICATED_ED25519_SEED)),
            )
            .replace(
                "1111111111111111111111111111111111111111111111111111111111111111",
                &hex(&x25519(&STORAGE_X25519_SEED).1),
            )
            .replace(
                "2222222222222222222222222222222222222222222222222222222222222222",
                &hex(&ed25519(&STORAGE_ED25519_SEED)),
            )
            .replace(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &hex(&x25519(&STORAGE2_X25519_SEED).1),
            )
            .replace(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &hex(&ed25519(&STORAGE2_ED25519_SEED)),
            );

        ServerConfig::from_toml_str(&document).expect("the failover configuration must parse")
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

    /// Keys and identity a mock storage node presents.
    #[derive(Clone, Copy)]
    struct MockNode {
        x25519_secret: [u8; 32],
        ed25519_seed: [u8; 32],
        sender_id: [u8; 16],
        label: &'static str,
    }

    fn node_two() -> MockNode {
        MockNode {
            x25519_secret: STORAGE2_X25519_SEED,
            ed25519_seed: STORAGE2_ED25519_SEED,
            sender_id: {
                let mut id = [0x00; 16];
                id[15] = 0x02;
                id
            },
            label: "storage-2",
        }
    }

    fn node_one() -> MockNode {
        MockNode {
            x25519_secret: STORAGE_X25519_SEED,
            ed25519_seed: STORAGE_ED25519_SEED,
            sender_id: STORAGE_ID,
            label: "storage-1",
        }
    }

    fn mock_storage(listener: TcpListener, reply: Reply, node: MockNode) -> thread::JoinHandle<()> {
        mock_storage_sequence(listener, vec![reply], node).0
    }

    fn mock_storage_sequence(
        listener: TcpListener,
        replies: Vec<Reply>,
        node: MockNode,
    ) -> (thread::JoinHandle<()>, Arc<AtomicU32>) {
        let accepted = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&accepted);
        let handle = thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().expect("the client must connect");
                counter.fetch_add(1, Ordering::Relaxed);
                let limit = std::num::NonZeroU32::new(4 * 1024 * 1024).expect("a non-zero limit");
                let body = read_frame(&mut stream, limit).expect("the request must arrive");
                let header_length =
                    external_header_length(&body).expect("the request header must be found");
                let request_header = ExternalHeader::decode(&body[..header_length], 64)
                    .expect("the header must parse");
                assert_eq!(request_header.sender_id, Some(DEDICATED_ID));
                assert_eq!(request_header.frame_number, 0);
                assert_eq!(request_header.upload_id, None);
                assert_eq!(request_header.responder_label, None);
                assert_eq!(request_header.bootstrap_signing_public_key, None);
                let mut sealed = Vec::with_capacity(4 + body.len());
                sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
                sealed.extend_from_slice(&body);
                let (storage_x25519, _) = x25519(&node.x25519_secret);
                let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
                let plaintext =
                    open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
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
                    Reply::Message(response) => (response, node.ed25519_seed),
                    Reply::ForeignSignature(response) => (response, FOREIGN_ED25519_SEED),
                    Reply::MalformedHeader => {
                        let frame =
                            seal_frame(&[0xFF; 4], &[0x80], &dedicated_x25519, &node.ed25519_seed)
                                .expect("the malformed response must seal");
                        stream
                            .write_all(&frame)
                            .expect("the malformed response must be written");
                        continue;
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
            }
        });

        (handle, accepted)
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

    #[derive(Clone)]
    struct ProposeReply {
        expected_tag: [u8; 32],
        installation_id: [u8; 16],
        code: u32,
        commit_index: u64,
        expected_verifier: Option<String>,
        new_verifier: Option<String>,
    }

    fn mock_storage_propose_sequence(
        listener: TcpListener,
        node: MockNode,
        replies: Vec<ProposeReply>,
        request_ids: Arc<std::sync::Mutex<Vec<[u8; 16]>>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().expect("the client must connect");
                let limit = std::num::NonZeroU32::new(4 * 1024 * 1024).expect("a non-zero limit");
                let body = read_frame(&mut stream, limit).expect("the request must arrive");
                let header_length =
                    external_header_length(&body).expect("the request header must be found");
                let request_header = ExternalHeader::decode(&body[..header_length], 64)
                    .expect("the header must parse");
                request_ids
                    .lock()
                    .expect("the request ids must lock")
                    .push(request_header.request_id);
                let mut sealed = Vec::with_capacity(4 + body.len());
                sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
                sealed.extend_from_slice(&body);
                let (storage_x25519, _) = x25519(&node.x25519_secret);
                let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
                let plaintext =
                    open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
                        .expect("the request must open");
                let request =
                    InterServerMessage::decode(&plaintext, 512).expect("the request must parse");
                match request {
                    InterServerMessage::RegistrationPropose {
                        login,
                        registration_request_id,
                        content_tag,
                        installation_id: sent,
                        x25519_key,
                        ed25519_key,
                        expected_verifier,
                        new_verifier,
                    } => {
                        assert_eq!(login, "alice");
                        assert_eq!(registration_request_id, [0x11; 16]);
                        assert_eq!(content_tag, reply.expected_tag);
                        assert_eq!(sent, reply.installation_id);
                        assert_eq!(x25519_key, [0x44; 32]);
                        assert_eq!(ed25519_key, [0x55; 32]);
                        assert_eq!(
                            expected_verifier.as_deref(),
                            reply.expected_verifier.as_deref()
                        );
                        assert_eq!(new_verifier.as_deref(), reply.new_verifier.as_deref());
                    }
                    _ => panic!("the request must be a propose"),
                }
                let response = InterServerMessage::RegistrationResult {
                    code: reply.code,
                    installation_id: (reply.code == 0).then_some(reply.installation_id),
                    commit_index: (reply.code == 0).then_some(reply.commit_index),
                };
                let payload = response.encode(512).expect("the response must encode");
                let header = ExternalHeader {
                    sender_id: Some(node.sender_id),
                    request_id: request_header.request_id,
                    frame_number: 0,
                    upload_id: None,
                    responder_label: Some(node.label.to_owned()),
                    bootstrap_signing_public_key: None,
                };
                let header_bytes = header.encode(64).expect("the response header must encode");
                let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
                let frame = seal_frame(
                    &header_bytes,
                    &payload,
                    &dedicated_x25519,
                    &node.ed25519_seed,
                )
                .expect("the response must seal");
                stream
                    .write_all(&frame)
                    .expect("the response must be written");
            }
        })
    }

    fn mock_storage_propose(
        listener: TcpListener,
        node: MockNode,
        reply: ProposeReply,
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
            let (storage_x25519, _) = x25519(&node.x25519_secret);
            let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
            let plaintext = open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
                .expect("the request must open");
            let request =
                InterServerMessage::decode(&plaintext, 512).expect("the request must parse");
            match request {
                InterServerMessage::RegistrationPropose {
                    login,
                    registration_request_id,
                    content_tag,
                    installation_id: sent,
                    x25519_key,
                    ed25519_key,
                    expected_verifier,
                    new_verifier,
                } => {
                    assert_eq!(login, "alice");
                    assert_eq!(registration_request_id, [0x11; 16]);
                    assert_eq!(content_tag, reply.expected_tag);
                    assert_eq!(sent, reply.installation_id);
                    assert_eq!(x25519_key, [0x44; 32]);
                    assert_eq!(ed25519_key, [0x55; 32]);
                    assert_eq!(
                        expected_verifier.as_deref(),
                        reply.expected_verifier.as_deref()
                    );
                    assert_eq!(new_verifier.as_deref(), reply.new_verifier.as_deref());
                }
                _ => panic!("the request must be a registration propose"),
            }

            let response = InterServerMessage::RegistrationResult {
                code: reply.code,
                installation_id: (reply.code == 0).then_some(reply.installation_id),
                commit_index: (reply.code == 0).then_some(reply.commit_index),
            };
            let payload = response.encode(512).expect("the response must encode");
            let header = ExternalHeader {
                sender_id: Some(node.sender_id),
                request_id: request_header.request_id,
                frame_number: 0,
                upload_id: None,
                responder_label: Some(node.label.to_owned()),
                bootstrap_signing_public_key: None,
            };
            let header_bytes = header.encode(64).expect("the response header must encode");
            let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
            let frame = seal_frame(
                &header_bytes,
                &payload,
                &dedicated_x25519,
                &node.ed25519_seed,
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
        propose_with_verifiers(
            expected_tag,
            installation_id,
            code,
            commit_index,
            None,
            Some("new-phc"),
        )
    }

    fn propose_with_verifiers(
        expected_tag: [u8; 32],
        installation_id: [u8; 16],
        code: u32,
        commit_index: u64,
        expected_verifier: Option<&str>,
        new_verifier: Option<&str>,
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
        let server = mock_storage_propose(
            listener,
            node_one(),
            ProposeReply {
                expected_tag,
                installation_id,
                code,
                commit_index,
                expected_verifier: expected_verifier.map(str::to_owned),
                new_verifier: new_verifier.map(str::to_owned),
            },
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id,
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier,
            new_verifier,
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
        let server = mock_storage(listener, reply, node_one());
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
    fn an_existing_account_propose_carries_the_expected_verifier() {
        let dir = TempDir::new("tag-existing");
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");

        let commit = propose_with_verifiers(expected_tag, [0x66; 16], 0, 4, Some("old-phc"), None)
            .expect("the existing-account propose must be committed");

        assert_eq!(commit.commit_index, 4);
    }

    #[test]
    fn a_propose_login_outside_the_configured_range_is_rejected_before_connecting() {
        let dir = TempDir::new("propose-login");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let config = config(9);
        let login = "a".repeat(65);
        let request = RegistrationProposeRequest {
            login: &login,
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        assert!(matches!(
            propose_registration(&config, &identity, &secret, &request),
            Err(InterServerClientError::LoginOutOfBounds {
                actual: 65,
                min: 1,
                max: 64
            })
        ));
    }

    #[test]
    fn a_propose_fails_over_to_the_next_storage_node() {
        let dir = TempDir::new("propose-failover");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_failover(closed_port(), port, 3, 1, 5_000);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let server = mock_storage_propose(
            listener,
            node_two(),
            ProposeReply {
                expected_tag,
                installation_id: [0x66; 16],
                code: 0,
                commit_index: 11,
                expected_verifier: None,
                new_verifier: Some("new-phc".to_owned()),
            },
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        let commit = propose_registration(&config, &identity, &secret, &request)
            .expect("the second storage node must commit");

        server.join().expect("the mock server must finish");
        assert_eq!(commit.commit_index, 11);
    }

    #[test]
    fn a_temporary_propose_rejection_retries_the_same_command_on_the_next_node() {
        let dir = TempDir::new("propose-temporary");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let first = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let first_port = first
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let second = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let second_port = second
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_two_nodes(first_port, second_port, 3);
        let reply = ProposeReply {
            expected_tag,
            installation_id: [0x66; 16],
            code: 0,
            commit_index: 12,
            expected_verifier: None,
            new_verifier: Some("new-phc".to_owned()),
        };
        let request_ids = Arc::new(std::sync::Mutex::new(Vec::new()));
        let node_a = mock_storage_propose_sequence(
            first,
            node_one(),
            vec![ProposeReply {
                code: 0x0007_0001,
                ..reply.clone()
            }],
            Arc::clone(&request_ids),
        );
        let node_b = mock_storage_propose_sequence(
            second,
            node_two(),
            vec![reply],
            Arc::clone(&request_ids),
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        let commit = propose_registration(&config, &identity, &secret, &request)
            .expect("the second node must commit the same command");

        node_a.join().expect("the first mock must finish");
        node_b.join().expect("the second mock must finish");
        assert_eq!(commit.commit_index, 12);
        let ids = request_ids.lock().expect("the request ids must lock");
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "each attempt must carry its own request id");
    }

    #[test]
    fn a_quorum_rejection_retries_the_same_command_on_the_next_node() {
        let dir = TempDir::new("propose-quorum");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let first = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let first_port = first
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let second = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let second_port = second
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_two_nodes(first_port, second_port, 3);
        let reply = ProposeReply {
            expected_tag,
            installation_id: [0x66; 16],
            code: 0,
            commit_index: 13,
            expected_verifier: None,
            new_verifier: Some("new-phc".to_owned()),
        };
        let request_ids = Arc::new(std::sync::Mutex::new(Vec::new()));
        let node_a = mock_storage_propose_sequence(
            first,
            node_one(),
            vec![ProposeReply {
                code: 0x0006_0001,
                ..reply.clone()
            }],
            Arc::clone(&request_ids),
        );
        let node_b = mock_storage_propose_sequence(
            second,
            node_two(),
            vec![reply],
            Arc::clone(&request_ids),
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        let commit = propose_registration(&config, &identity, &secret, &request)
            .expect("the quorum rejection must be retried");

        node_a.join().expect("the first mock must finish");
        node_b.join().expect("the second mock must finish");
        assert_eq!(commit.commit_index, 13);
        let ids = request_ids.lock().expect("the request ids must lock");
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "each attempt must carry its own request id");
    }

    #[test]
    fn a_permanent_propose_rejection_does_not_reach_the_next_node() {
        let dir = TempDir::new("propose-permanent");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let first = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let first_port = first
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let second = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let second_port = second
            .local_addr()
            .expect("the listener must have an address")
            .port();
        second
            .set_nonblocking(true)
            .expect("the second listener must be non-blocking");
        let config = config_with_two_nodes(first_port, second_port, 3);
        let node_a = mock_storage_propose_sequence(
            first,
            node_one(),
            vec![ProposeReply {
                expected_tag,
                installation_id: [0x66; 16],
                code: 0x0003_0001,
                commit_index: 0,
                expected_verifier: None,
                new_verifier: Some("new-phc".to_owned()),
            }],
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        let error = propose_registration(&config, &identity, &secret, &request)
            .expect_err("a permanent rejection must fail");

        node_a.join().expect("the first mock must finish");
        assert!(matches!(
            error,
            InterServerClientError::Rejected(0x0003_0001)
        ));
        assert!(
            matches!(second.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "the second node must not receive a retried command"
        );
    }

    #[test]
    fn the_configured_attempt_count_is_exact() {
        let config = config_with_failover(closed_port(), closed_port(), 3, 1, 5_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2_000);
        let mut calls = 0;

        let result: Result<()> = with_registration_budget(&config, deadline, |_, _| {
            calls += 1;
            Err(InterServerClientError::Rejected(0x0007_0001))
        });

        assert!(matches!(
            result,
            Err(InterServerClientError::Rejected(0x0007_0001))
        ));
        assert_eq!(calls, 3, "the attempt count must be exact");
    }

    #[test]
    fn exactly_one_attempt_is_made_when_the_budget_allows_one() {
        let dir = TempDir::new("one-attempt");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let expected_tag = content_tag(
            &secret,
            "alice",
            "secret-password",
            &[0x44; 32],
            &[0x55; 32],
        )
        .expect("the tag must be computed");
        let first = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let first_port = first
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let second = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let second_port = second
            .local_addr()
            .expect("the listener must have an address")
            .port();
        second
            .set_nonblocking(true)
            .expect("the second listener must be non-blocking");
        let config = config_with_two_nodes(first_port, second_port, 1);
        let node_a = mock_storage_propose_sequence(
            first,
            node_one(),
            vec![ProposeReply {
                expected_tag,
                installation_id: [0x66; 16],
                code: 0x0007_0001,
                commit_index: 0,
                expected_verifier: None,
                new_verifier: Some("new-phc".to_owned()),
            }],
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        let request = RegistrationProposeRequest {
            login: "alice",
            password: "secret-password",
            installation_id: [0x66; 16],
            request_id: [0x11; 16],
            x25519_key: [0x44; 32],
            ed25519_key: [0x55; 32],
            expected_verifier: None,
            new_verifier: Some("new-phc"),
        };

        assert!(matches!(
            propose_registration(&config, &identity, &secret, &request),
            Err(InterServerClientError::Rejected(0x0007_0001))
        ));

        node_a.join().expect("the first mock must finish");
        assert!(
            matches!(second.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "one attempt must not reach the second node"
        );
    }

    #[test]
    fn a_stalling_node_is_bounded_by_the_attempt_deadline() {
        let dir = TempDir::new("stalling");
        let identity = identity(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_two_nodes(port, closed_port(), 1);
        let staller = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("the client must connect");
            thread::sleep(std::time::Duration::from_millis(3_000));
            drop(stream);
        });
        let started = std::time::Instant::now();

        let error = lookup_registry(&config, &identity, Some("alice"), None, None)
            .expect_err("a stalling node must time out");

        let elapsed = started.elapsed();
        staller.join().expect("the stalling node must finish");
        assert!(
            matches!(error, InterServerClientError::Frame(_)),
            "the read must time out before an attempt deadline"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(1_500),
            "the attempt must be bounded, took {elapsed:?}"
        );
    }

    #[test]
    fn a_temporary_rejection_survives_an_expired_budget() {
        let config = config_with_failover(closed_port(), closed_port(), 100, 1, 1_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(30);

        let result: Result<()> = with_registration_budget(&config, deadline, |_, _| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            Err(InterServerClientError::Rejected(0x0006_0001))
        });

        assert!(matches!(
            result,
            Err(InterServerClientError::Rejected(0x0006_0001))
        ));
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
    fn a_permanent_rejection_is_not_retried() {
        let dir = TempDir::new("permanent");
        let identity = identity(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_failover(closed_port(), port, 3, 1, 5_000);
        let response = MockResponse {
            sender_id: Some(node_two().sender_id),
            responder_label: Some(node_two().label.to_owned()),
            code: 0x0001_0002,
            snapshot: None,
            ..MockResponse::empty()
        };
        let (server, accepted) =
            mock_storage_sequence(listener, vec![Reply::Message(response)], node_two());

        let error = lookup_registry(&config, &identity, Some("alice"), None, None)
            .expect_err("a permanent rejection must fail");

        server.join().expect("the mock server must finish");
        assert!(matches!(
            error,
            InterServerClientError::Rejected(0x0001_0002)
        ));
        assert_eq!(
            accepted.load(Ordering::Relaxed),
            1,
            "a permanent rejection must not be retried"
        );
    }

    #[test]
    fn a_temporary_rejection_is_retried() {
        let dir = TempDir::new("temporary");
        let identity = identity(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config_with_failover(closed_port(), port, 4, 1, 5_000);
        let temporary = MockResponse {
            sender_id: Some(node_two().sender_id),
            responder_label: Some(node_two().label.to_owned()),
            code: 0x0007_0001,
            snapshot: None,
            ..MockResponse::empty()
        };
        let snapshot = RegistrySnapshot {
            read_index: 9,
            account: None,
            registration: None,
            installation: None,
        };
        let recovery = MockResponse {
            sender_id: Some(node_two().sender_id),
            responder_label: Some(node_two().label.to_owned()),
            ..MockResponse::snapshot(snapshot.clone())
        };
        let (server, accepted) = mock_storage_sequence(
            listener,
            vec![Reply::Message(temporary), Reply::Message(recovery)],
            node_two(),
        );

        let found = lookup_registry(&config, &identity, Some("alice"), None, None)
            .expect("the temporary rejection must be retried");

        server.join().expect("the mock server must finish");
        assert_eq!(found, snapshot);
        assert_eq!(
            accepted.load(Ordering::Relaxed),
            2,
            "the temporary rejection must be retried once"
        );
    }

    #[test]
    fn a_lookup_fails_over_to_the_next_storage_node() {
        let dir = TempDir::new("failover");
        let identity = identity(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let snapshot = RegistrySnapshot {
            read_index: 7,
            account: None,
            registration: None,
            installation: None,
        };
        let response = MockResponse {
            sender_id: Some(node_two().sender_id),
            responder_label: Some(node_two().label.to_owned()),
            ..MockResponse::snapshot(snapshot.clone())
        };
        let server = mock_storage(listener, Reply::Message(response), node_two());
        let config = config_with_failover(closed_port(), port, 2, 1, 5_000);

        let found = lookup_registry(&config, &identity, Some("alice"), None, None)
            .expect("the second storage node must answer");

        server.join().expect("the mock server must finish");
        assert_eq!(found, snapshot);
    }

    #[test]
    fn a_lookup_gives_up_after_the_configured_attempts() {
        let dir = TempDir::new("give-up");
        let identity = identity(&dir);
        let config = config_with_failover(closed_port(), closed_port(), 2, 1, 5_000);

        assert!(matches!(
            lookup_registry(&config, &identity, Some("alice"), None, None),
            Err(InterServerClientError::Connect { .. })
        ));
    }

    #[test]
    fn a_lookup_stops_at_the_total_deadline() {
        let dir = TempDir::new("deadline");
        let identity = identity(&dir);
        let config = config_with_failover(closed_port(), closed_port(), 10, 400, 1_000);

        assert!(matches!(
            lookup_registry(&config, &identity, Some("alice"), None, None),
            Err(InterServerClientError::Deadline { millis: 1_000 })
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
    fn the_attempt_budget_never_exceeds_the_total_deadline() {
        let config = config_with_failover(closed_port(), closed_port(), 1, 1, 1_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(40);
        let mut seen = None;

        let result: Result<()> = with_registration_budget(&config, deadline, |_, budget| {
            seen = Some(budget);
            Ok(())
        });

        assert!(result.is_ok());
        assert!(seen.expect("the attempt must run") <= std::time::Duration::from_millis(40));
    }

    #[test]
    fn an_attempt_after_the_deadline_is_not_started() {
        let config = config_with_failover(closed_port(), closed_port(), 2, 1, 1_000);
        let deadline = std::time::Instant::now();
        let mut ran = false;

        let result: Result<()> = with_registration_budget(&config, deadline, |_, _| {
            ran = true;
            Ok(())
        });

        assert!(matches!(
            result,
            Err(InterServerClientError::Deadline { .. })
        ));
        assert!(!ran);
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
