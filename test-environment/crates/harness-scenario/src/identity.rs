//! Node identity consistency checks for the static manifest and topology answer.
//!
//! The storage-node UUIDs published in the manifest must be unique, must match
//! the topology answer, and must identify the sender of a signed node message
//! (`TST-024 / 1`). Only the manifest scalar fields needed for identity checks
//! are read here; host and key validation belong to the component manifests.

use std::collections::{BTreeMap, BTreeSet};

use harness_wire::header::ExternalFrameHeader;
use harness_wire::topology::TopologyAnswer;
use serde::Deserialize;

const UUID_LENGTH: usize = 36;
const UUID_BYTE_LENGTH: usize = 16;
const UUID_HYPHEN_POSITIONS: [usize; 4] = [8, 13, 18, 23];

/// A storage-node identity published in the static manifest (`TST-024`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIdentity {
    /// The node's stable sender UUID.
    pub sender_id: [u8; 16],
    /// The node's stable label.
    pub label: String,
}

/// Reasons node identities are inconsistent or the manifest cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The manifest text is not valid TOML.
    #[error("manifest is not valid TOML: {0}")]
    Manifest(#[from] toml::de::Error),
    /// A manifest sender_id is not a hyphenated UUID.
    #[error("sender_id {0:?} is not a hyphenated UUID")]
    InvalidSenderId(String),
    /// The manifest repeats a sender UUID.
    #[error("duplicate sender_id {0:?}")]
    DuplicateSenderId([u8; 16]),
    /// The manifest repeats a label.
    #[error("duplicate label {0:?}")]
    DuplicateLabel(String),
    /// A manifest storage node is absent from the topology answer.
    #[error("storage node {label:?} from the manifest is missing from the topology answer")]
    MissingFromTopology { label: String },
    /// A topology node does not exist in the manifest.
    #[error("topology node {sender_id:?} is not in the manifest")]
    UnknownInTopology { sender_id: [u8; 16] },
    /// The topology answer repeats a sender UUID.
    #[error("topology repeats sender_id {0:?}")]
    DuplicateTopologySenderId([u8; 16]),
    /// A topology node label differs from the manifest.
    #[error("topology label for {sender_id:?} differs from the manifest")]
    LabelMismatch { sender_id: [u8; 16] },
    /// A node message has no external sender UUID.
    #[error("a node message must carry a sender_id")]
    MissingMessageSender,
    /// A node message sender does not exist in the manifest.
    #[error("message sender {sender_id:?} is not in the manifest")]
    UnknownMessageSender { sender_id: [u8; 16] },
}

#[derive(Debug, Deserialize)]
struct ManifestDocument {
    #[serde(default)]
    storage: Vec<StorageRow>,
}

#[derive(Debug, Deserialize)]
struct StorageRow {
    sender_id: String,
    label: String,
}

/// Reads the `[[storage]]` sender_id/label pairs from a manifest TOML document.
///
/// Only the published scalar subset needed for identity checks is read; unknown
/// fields, the `[dedicated]` table and validation of hosts/keys are out of scope.
pub fn manifest_storage_identities(text: &str) -> Result<Vec<NodeIdentity>, IdentityError> {
    let document: ManifestDocument = toml::from_str(text)?;

    let identities = document
        .storage
        .into_iter()
        .map(|row| {
            Ok(NodeIdentity {
                sender_id: parse_uuid(&row.sender_id)?,
                label: row.label,
            })
        })
        .collect::<Result<Vec<_>, IdentityError>>()?;

    validate_manifest(&identities)?;
    Ok(identities)
}

/// Verifies storage UUID uniqueness and manifest/topology consistency (`TST-024`).
pub fn verify_topology_identities(
    manifest: &[NodeIdentity],
    topology: &TopologyAnswer,
) -> Result<(), IdentityError> {
    validate_manifest(manifest)?;

    let by_id: BTreeMap<[u8; 16], &str> = manifest
        .iter()
        .map(|node| (node.sender_id, node.label.as_str()))
        .collect();

    let mut seen = BTreeSet::new();
    for node in &topology.nodes {
        let Some(expected_label) = by_id.get(&node.sender_id) else {
            return Err(IdentityError::UnknownInTopology {
                sender_id: node.sender_id,
            });
        };
        if !seen.insert(node.sender_id) {
            return Err(IdentityError::DuplicateTopologySenderId(node.sender_id));
        }
        if *expected_label != node.label {
            return Err(IdentityError::LabelMismatch {
                sender_id: node.sender_id,
            });
        }
    }

    for node in manifest {
        if !seen.contains(&node.sender_id) {
            return Err(IdentityError::MissingFromTopology {
                label: node.label.clone(),
            });
        }
    }

    Ok(())
}

/// Verifies that a signed node message's external `sender_id` belongs to the manifest.
pub fn verify_message_sender(
    manifest: &[NodeIdentity],
    header: &ExternalFrameHeader,
) -> Result<(), IdentityError> {
    let sender_id = header
        .sender_id
        .ok_or(IdentityError::MissingMessageSender)?;

    if manifest.iter().any(|node| node.sender_id == sender_id) {
        Ok(())
    } else {
        Err(IdentityError::UnknownMessageSender { sender_id })
    }
}

fn validate_manifest(manifest: &[NodeIdentity]) -> Result<(), IdentityError> {
    for (index, node) in manifest.iter().enumerate() {
        let earlier = &manifest[..index];
        if earlier
            .iter()
            .any(|other| other.sender_id == node.sender_id)
        {
            return Err(IdentityError::DuplicateSenderId(node.sender_id));
        }
        if earlier.iter().any(|other| other.label == node.label) {
            return Err(IdentityError::DuplicateLabel(node.label.clone()));
        }
    }

    Ok(())
}

fn parse_uuid(value: &str) -> Result<[u8; UUID_BYTE_LENGTH], IdentityError> {
    let bytes = value.as_bytes();
    if bytes.len() != UUID_LENGTH
        || UUID_HYPHEN_POSITIONS
            .iter()
            .any(|position| bytes.get(*position) != Some(&b'-'))
    {
        return Err(IdentityError::InvalidSenderId(value.to_owned()));
    }

    let mut sender_id = [0u8; UUID_BYTE_LENGTH];
    let mut pending_nibble = None;
    let mut index = 0;

    for (offset, byte) in bytes.iter().enumerate() {
        if UUID_HYPHEN_POSITIONS.contains(&offset) {
            continue;
        }

        let digit =
            hex_digit(*byte).ok_or_else(|| IdentityError::InvalidSenderId(value.to_owned()))?;

        if let Some(high) = pending_nibble {
            let Some(slot) = sender_id.get_mut(index) else {
                return Err(IdentityError::InvalidSenderId(value.to_owned()));
            };
            *slot = (high << 4) | digit;
            index += 1;
            pending_nibble = None;
        } else {
            pending_nibble = Some(digit);
        }
    }

    if index != UUID_BYTE_LENGTH {
        return Err(IdentityError::InvalidSenderId(value.to_owned()));
    }

    Ok(sender_id)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityError, NodeIdentity, manifest_storage_identities, parse_uuid,
        verify_message_sender, verify_topology_identities,
    };
    use harness_wire::header::ExternalFrameHeader;
    use harness_wire::host::CanonicalHost;
    use harness_wire::topology::{NodeEntry, TopologyAnswer};

    const MANIFEST: &str = r#"
[dedicated]
sender_id = "33333333-3333-4333-8333-333333333333"
label = "dedicated"

[[storage]]
sender_id = "11111111-1111-4111-8111-111111111111"
label = "storage-a"

[[storage]]
sender_id = "22222222-2222-4222-8222-222222222222"
label = "storage-b"
"#;

    fn identity(sender_id: [u8; 16], label: &str) -> NodeIdentity {
        NodeIdentity {
            sender_id,
            label: label.to_owned(),
        }
    }

    fn node(identity: &NodeIdentity) -> NodeEntry {
        NodeEntry {
            sender_id: identity.sender_id,
            label: identity.label.clone(),
            host: CanonicalHost::parse("192.0.2.10").expect("valid host"),
            port: 7001,
            x25519_public_key: [0; 32],
            ed25519_public_key: [0; 32],
        }
    }

    fn message_header(sender_id: Option<[u8; 16]>) -> ExternalFrameHeader {
        ExternalFrameHeader {
            sender_id,
            request_id: [0; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        }
    }

    #[test]
    fn parses_storage_identities_in_manifest_order() {
        let identities = manifest_storage_identities(MANIFEST).expect("manifest is valid");

        assert_eq!(
            identities,
            vec![
                identity(
                    parse_uuid("11111111-1111-4111-8111-111111111111").expect("valid uuid"),
                    "storage-a",
                ),
                identity(
                    parse_uuid("22222222-2222-4222-8222-222222222222").expect("valid uuid"),
                    "storage-b",
                ),
            ]
        );
    }

    #[test]
    fn rejects_malformed_manifest_toml() {
        let error = manifest_storage_identities("[[storage]\nsender_id = \"x\"\n")
            .expect_err("manifest is invalid");

        assert!(matches!(error, IdentityError::Manifest(_)));
    }

    #[test]
    fn rejects_invalid_manifest_uuids() {
        for value in [
            "11111111-1111-4111-8111-11111111111",
            "1111-11111111-4111-8111-111111111111",
            "11111111-1111-4111-8111-11111111111g",
        ] {
            assert!(matches!(
                parse_uuid(value),
                Err(IdentityError::InvalidSenderId(_))
            ));
        }
    }

    #[test]
    fn rejects_duplicate_manifest_sender_id() {
        let manifest = r#"
[[storage]]
sender_id = "11111111-1111-4111-8111-111111111111"
label = "storage-a"

[[storage]]
sender_id = "11111111-1111-4111-8111-111111111111"
label = "storage-b"
"#;

        assert!(matches!(
            manifest_storage_identities(manifest),
            Err(IdentityError::DuplicateSenderId(id)) if id == [0x11; 16]
        ));
    }

    #[test]
    fn rejects_duplicate_manifest_label() {
        let manifest = r#"
[[storage]]
sender_id = "11111111-1111-4111-8111-111111111111"
label = "storage-a"

[[storage]]
sender_id = "22222222-2222-4222-8222-222222222222"
label = "storage-a"
"#;

        assert!(matches!(
            manifest_storage_identities(manifest),
            Err(IdentityError::DuplicateLabel(label)) if label == "storage-a"
        ));
    }

    #[test]
    fn accepts_matching_manifest_and_topology() {
        let manifest = vec![
            identity([1; 16], "storage-a"),
            identity([2; 16], "storage-b"),
        ];
        let topology = TopologyAnswer {
            nodes: vec![node(&manifest[0]), node(&manifest[1])],
        };

        assert!(verify_topology_identities(&manifest, &topology).is_ok());
    }

    #[test]
    fn accepts_empty_manifest_and_topology() {
        let topology = TopologyAnswer { nodes: Vec::new() };

        assert!(verify_topology_identities(&[], &topology).is_ok());
    }

    #[test]
    fn rejects_topology_label_mismatch() {
        let manifest = vec![identity([1; 16], "storage-a")];
        let topology = TopologyAnswer {
            nodes: vec![NodeEntry {
                label: "wrong".to_owned(),
                ..node(&manifest[0])
            }],
        };

        assert!(matches!(
            verify_topology_identities(&manifest, &topology),
            Err(IdentityError::LabelMismatch { sender_id }) if sender_id == [1; 16]
        ));
    }

    #[test]
    fn rejects_manifest_node_missing_from_topology() {
        let manifest = vec![
            identity([1; 16], "storage-a"),
            identity([2; 16], "storage-b"),
        ];
        let topology = TopologyAnswer {
            nodes: vec![node(&manifest[0])],
        };

        assert!(matches!(
            verify_topology_identities(&manifest, &topology),
            Err(IdentityError::MissingFromTopology { label }) if label == "storage-b"
        ));
    }

    #[test]
    fn rejects_unknown_topology_node() {
        let manifest = vec![identity([1; 16], "storage-a")];
        let unknown = identity([2; 16], "storage-b");
        let topology = TopologyAnswer {
            nodes: vec![node(&unknown)],
        };

        assert!(matches!(
            verify_topology_identities(&manifest, &topology),
            Err(IdentityError::UnknownInTopology { sender_id }) if sender_id == [2; 16]
        ));
    }

    #[test]
    fn rejects_duplicate_topology_sender_id() {
        let manifest = vec![identity([1; 16], "storage-a")];
        let topology = TopologyAnswer {
            nodes: vec![node(&manifest[0]), node(&manifest[0])],
        };

        assert!(matches!(
            verify_topology_identities(&manifest, &topology),
            Err(IdentityError::DuplicateTopologySenderId(id)) if id == [1; 16]
        ));
    }

    #[test]
    fn accepts_known_message_sender() {
        let manifest = vec![identity([1; 16], "storage-a")];

        assert!(verify_message_sender(&manifest, &message_header(Some([1; 16]))).is_ok());
    }

    #[test]
    fn rejects_unknown_message_sender() {
        let manifest = vec![identity([1; 16], "storage-a")];

        assert!(matches!(
            verify_message_sender(&manifest, &message_header(Some([2; 16]))),
            Err(IdentityError::UnknownMessageSender { sender_id }) if sender_id == [2; 16]
        ));
    }

    #[test]
    fn rejects_missing_message_sender() {
        let manifest = vec![identity([1; 16], "storage-a")];

        assert!(matches!(
            verify_message_sender(&manifest, &message_header(None)),
            Err(IdentityError::MissingMessageSender)
        ));
    }
}
