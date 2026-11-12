//! Registration decision from a registry lookup to a propose command.
//!
//! A registration first reads the linearizable registry (`INT-NET-101 / 1`).
//! An exact repeat of a recorded request returns the recorded installation
//! identifier without changing state, a reused identifier with different
//! content is a conflict (`INT-NET-105 / 1`), an existing account requires the
//! password to match its PHC verifier before an installation is added
//! (`INT-NET-110 / 1`), and a new account is proposed with a freshly derived
//! verifier (`INT-NET-103 / 1`, `INT-NET-109 / 1`).

use thiserror::Error;

use crate::config::ServerConfig;
use crate::identity::ServerIdentity;
use crate::inter_server_client::{
    lookup_registry, propose_registration, InterServerClientError, RegistrationProposeRequest,
};
use crate::password::{PasswordError, PasswordVerifier};
use crate::registration_tag::{content_tag, RegistrationSecret, RegistrationSecretError};
use crate::wire::IDENTIFIER_BYTES;

/// A committed registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommittedRegistration {
    /// Installation identifier of the committed registration.
    pub installation_id: [u8; IDENTIFIER_BYTES],
    /// Raft index of the commit, or the snapshot index of a recorded replay.
    pub commit_index: u64,
}

/// Inputs of one registration attempt.
pub struct RegistrationInput<'a> {
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
}

/// Errors a registration attempt can end with.
#[derive(Debug, Error)]
pub enum RegistrationFlowError {
    /// The registry lookup or the propose exchange failed.
    #[error(transparent)]
    Exchange(#[from] InterServerClientError),
    /// The content tag could not be computed.
    #[error(transparent)]
    Tag(#[from] RegistrationSecretError),
    /// The password does not match the existing account.
    #[error("the password does not match the existing account")]
    PasswordRejected,
    /// The request identifier was recorded with different content.
    #[error("the request identifier was recorded with different content")]
    RequestIdConflict,
    /// Deriving or checking the verifier failed.
    #[error(transparent)]
    Password(#[from] PasswordError),
    /// The committed installation differs from the proposed one.
    #[error("the storage node committed another installation than the proposed one")]
    UnexpectedInstallation,
}

/// Run one registration: look the registry up and commit the appropriate command.
pub(crate) fn register(
    config: &ServerConfig,
    identity: &ServerIdentity,
    secret: &RegistrationSecret,
    input: &RegistrationInput<'_>,
) -> Result<CommittedRegistration, RegistrationFlowError> {
    let tag = content_tag(
        secret,
        input.login,
        input.password,
        &input.x25519_key,
        &input.ed25519_key,
    )?;
    let snapshot = lookup_registry(
        config,
        identity,
        Some(input.login),
        Some(input.request_id),
        Some(input.installation_id),
    )?;

    if let Some(recorded) = &snapshot.registration {
        if recorded.content_tag == tag {
            return Ok(CommittedRegistration {
                installation_id: recorded.installation_id,
                commit_index: snapshot.read_index,
            });
        }

        return Err(RegistrationFlowError::RequestIdConflict);
    }

    let (expected_verifier, new_verifier) = match &snapshot.account {
        Some(account) => {
            let verifier = PasswordVerifier::parse(&account.verifier)?;
            if !verifier.verify(input.password)? {
                return Err(RegistrationFlowError::PasswordRejected);
            }
            (Some(account.verifier.clone()), None)
        }
        None => (
            None,
            Some(
                PasswordVerifier::derive(input.password, config.password.params())?
                    .as_str()
                    .to_owned(),
            ),
        ),
    };

    let request = RegistrationProposeRequest {
        login: input.login,
        password: input.password,
        installation_id: input.installation_id,
        request_id: input.request_id,
        x25519_key: input.x25519_key,
        ed25519_key: input.ed25519_key,
        expected_verifier: expected_verifier.as_deref(),
        new_verifier: new_verifier.as_deref(),
    };
    let commit = propose_registration(config, identity, secret, &request)?;
    if commit.installation_id != input.installation_id {
        return Err(RegistrationFlowError::UnexpectedInstallation);
    }

    Ok(CommittedRegistration {
        installation_id: commit.installation_id,
        commit_index: commit.commit_index,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::thread;

    use hpke::kem::X25519HkdfSha256;
    use hpke::Serializable;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;
    use crate::config::ServerConfig;
    use crate::frame::read_frame;
    use crate::frame_seal::{open_frame, seal_frame};
    use crate::wire::{
        AccountRecord, ExternalHeader, InstallationRecord, InterServerMessage, RegistrySnapshot,
    };

    const FIXTURE: &str = include_str!("../tests/fixtures/valid_server.toml");
    const STORAGE_ID: [u8; 16] = {
        let mut id = [0x00; 16];
        id[15] = 0x01;
        id
    };
    const DEDICATED_X25519_SEED: [u8; 32] = [0x31; 32];
    const DEDICATED_ED25519_SEED: [u8; 32] = [0x41; 32];
    const STORAGE_X25519_SEED: [u8; 32] = [0x51; 32];
    const STORAGE_ED25519_SEED: [u8; 32] = [0x61; 32];
    const LOGIN: &str = "alice";
    const PASSWORD: &str = "secret-password";
    const X25519_KEY: [u8; 32] = [0x44; 32];
    const ED25519_KEY: [u8; 32] = [0x55; 32];
    const INSTALLATION: [u8; 16] = [0x66; 16];
    const REQUEST: [u8; 16] = [0x11; 16];

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
                "homecloud-flow-{name}-{}-{}",
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

    fn config(port: u16) -> ServerConfig {
        let (_, dedicated_x25519) = x25519(&DEDICATED_X25519_SEED);
        let dedicated_ed25519 = ed25519(&DEDICATED_ED25519_SEED);
        let (_, storage_x25519) = x25519(&STORAGE_X25519_SEED);
        let storage_ed25519 = ed25519(&STORAGE_ED25519_SEED);
        let document = FIXTURE
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

        ServerConfig::from_toml_str(&document).expect("the test configuration must parse")
    }

    /// One storage-node reply, either a lookup snapshot or a propose result.
    enum Reply {
        Snapshot(RegistrySnapshot),
        Commit {
            installation_id: [u8; 16],
            commit_index: u64,
        },
        Rejected(u32),
    }

    fn read_request(stream: &mut TcpStream) -> (ExternalHeader, InterServerMessage) {
        let limit = std::num::NonZeroU32::new(4 * 1024 * 1024).expect("a non-zero limit");
        let body = read_frame(stream, limit).expect("the request must arrive");
        let header_length = {
            let mut cursor = body.as_slice();
            let _header: ciborium::Value =
                ciborium::de::from_reader(&mut cursor).expect("the request header must parse");
            body.len() - cursor.len()
        };
        let header =
            ExternalHeader::decode(&body[..header_length], 64).expect("the header must parse");
        let mut sealed = Vec::with_capacity(4 + body.len());
        sealed.extend_from_slice(&(body.len() as u32).to_be_bytes());
        sealed.extend_from_slice(&body);
        let (storage_x25519, _) = x25519(&STORAGE_X25519_SEED);
        let dedicated_signing = ed25519(&DEDICATED_ED25519_SEED);
        let plaintext = open_frame(&sealed, header_length, &storage_x25519, &dedicated_signing)
            .expect("the request must open");
        let message = InterServerMessage::decode(&plaintext, 512).expect("the request must parse");

        (header, message)
    }

    fn reply(stream: &mut TcpStream, request: &ExternalHeader, message: InterServerMessage) {
        let payload = message.encode(512).expect("the response must encode");
        let header = ExternalHeader {
            sender_id: Some(STORAGE_ID),
            request_id: request.request_id,
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
    }

    fn serve(listener: TcpListener, proposals: Vec<Reply>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for reply_kind in proposals {
                let (mut stream, _) = listener.accept().expect("the client must connect");
                let (header, request) = read_request(&mut stream);
                let response = match reply_kind {
                    Reply::Snapshot(snapshot) => InterServerMessage::RegistryLookupResult {
                        code: 0,
                        snapshot: Some(snapshot),
                    },
                    Reply::Commit {
                        installation_id,
                        commit_index,
                    } => match request {
                        InterServerMessage::RegistrationPropose {
                            installation_id: sent,
                            new_verifier,
                            expected_verifier,
                            ..
                        } => {
                            assert_eq!(sent, installation_id);
                            assert_eq!(new_verifier.is_some(), expected_verifier.is_none());
                            InterServerMessage::RegistrationResult {
                                code: 0,
                                installation_id: Some(installation_id),
                                commit_index: Some(commit_index),
                            }
                        }
                        _ => panic!("the request must be a propose"),
                    },
                    Reply::Rejected(code) => InterServerMessage::RegistrationResult {
                        code,
                        installation_id: None,
                        commit_index: None,
                    },
                };
                reply(&mut stream, &header, response);
            }
        })
    }

    fn input<'a>(login: &'a str, password: &'a str) -> RegistrationInput<'a> {
        RegistrationInput {
            login,
            password,
            installation_id: INSTALLATION,
            request_id: REQUEST,
            x25519_key: X25519_KEY,
            ed25519_key: ED25519_KEY,
        }
    }

    fn run(
        proposals: Vec<Reply>,
        login: &str,
        password: &str,
    ) -> Result<CommittedRegistration, RegistrationFlowError> {
        let dir = TempDir::new("flow");
        let identity = identity(&dir);
        let secret = secret(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener must bind");
        let port = listener
            .local_addr()
            .expect("the listener must have an address")
            .port();
        let config = config(port);
        let server = serve(listener, proposals);
        let result = register(&config, &identity, &secret, &input(login, password));
        server.join().expect("the mock server must finish");

        result
    }

    fn empty_snapshot(read_index: u64) -> RegistrySnapshot {
        RegistrySnapshot {
            read_index,
            account: None,
            registration: None,
            installation: None,
        }
    }

    fn tag(login: &str, password: &str) -> [u8; 32] {
        let dir = TempDir::new("tag");
        let secret = secret(&dir);
        content_tag(&secret, login, password, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed")
    }

    #[test]
    fn a_new_account_is_proposed_and_committed() {
        let outcome = run(
            vec![
                Reply::Snapshot(empty_snapshot(3)),
                Reply::Commit {
                    installation_id: INSTALLATION,
                    commit_index: 8,
                },
            ],
            LOGIN,
            PASSWORD,
        )
        .expect("the registration must commit");

        assert_eq!(
            outcome,
            CommittedRegistration {
                installation_id: INSTALLATION,
                commit_index: 8
            }
        );
    }

    #[test]
    fn a_replay_with_the_recorded_tag_returns_the_recorded_installation() {
        let recorded = crate::wire::RegistrationRecord {
            content_tag: tag(LOGIN, PASSWORD),
            installation_id: INSTALLATION,
        };
        let snapshot = RegistrySnapshot {
            read_index: 12,
            account: None,
            registration: Some(recorded),
            installation: None,
        };

        let outcome = run(vec![Reply::Snapshot(snapshot)], LOGIN, PASSWORD)
            .expect("a replay must return the recorded result");

        assert_eq!(
            outcome,
            CommittedRegistration {
                installation_id: INSTALLATION,
                commit_index: 12
            }
        );
    }

    #[test]
    fn a_replay_with_another_tag_is_a_request_id_conflict() {
        let recorded = crate::wire::RegistrationRecord {
            content_tag: [0x99; 32],
            installation_id: INSTALLATION,
        };
        let snapshot = RegistrySnapshot {
            read_index: 12,
            account: None,
            registration: Some(recorded),
            installation: None,
        };

        assert!(matches!(
            run(vec![Reply::Snapshot(snapshot)], LOGIN, PASSWORD),
            Err(RegistrationFlowError::RequestIdConflict)
        ));
    }

    #[test]
    fn an_existing_account_with_the_matching_password_is_proposed() {
        let verifier = PasswordVerifier::derive(PASSWORD, config(1).password.params())
            .expect("the verifier must be derived");
        let snapshot = RegistrySnapshot {
            read_index: 3,
            account: Some(AccountRecord {
                verifier: verifier.as_str().to_owned(),
            }),
            registration: None,
            installation: Some(InstallationRecord {
                login: LOGIN.to_owned(),
                installation_id: [0x77; 16],
                x25519_key: X25519_KEY,
                ed25519_key: ED25519_KEY,
            }),
        };

        let outcome = run(
            vec![
                Reply::Snapshot(snapshot),
                Reply::Commit {
                    installation_id: INSTALLATION,
                    commit_index: 9,
                },
            ],
            LOGIN,
            PASSWORD,
        )
        .expect("the additional installation must commit");

        assert_eq!(outcome.commit_index, 9);
    }

    #[test]
    fn an_existing_account_with_another_password_is_rejected() {
        let verifier = PasswordVerifier::derive(PASSWORD, config(1).password.params())
            .expect("the verifier must be derived");
        let snapshot = RegistrySnapshot {
            read_index: 3,
            account: Some(AccountRecord {
                verifier: verifier.as_str().to_owned(),
            }),
            registration: None,
            installation: None,
        };

        assert!(matches!(
            run(vec![Reply::Snapshot(snapshot)], LOGIN, "another-password"),
            Err(RegistrationFlowError::PasswordRejected)
        ));
    }

    #[test]
    fn a_committed_installation_that_differs_from_the_proposal_is_rejected() {
        assert!(matches!(
            run(
                vec![
                    Reply::Snapshot(empty_snapshot(3)),
                    Reply::Commit {
                        installation_id: [0x88; 16],
                        commit_index: 9,
                    },
                ],
                LOGIN,
                PASSWORD,
            ),
            Err(RegistrationFlowError::UnexpectedInstallation)
        ));
    }

    #[test]
    fn a_storage_rejection_is_reported() {
        assert!(matches!(
            run(
                vec![
                    Reply::Snapshot(empty_snapshot(3)),
                    Reply::Rejected(0x0003_0001),
                ],
                LOGIN,
                PASSWORD,
            ),
            Err(RegistrationFlowError::Exchange(
                InterServerClientError::Rejected(0x0003_0001)
            ))
        ));
    }
}
