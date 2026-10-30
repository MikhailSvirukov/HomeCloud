//! Static TOML manifest.
//!
//! SRS: `CFG-CL-001 / 1`–`CFG-CL-005 / 1`, `CFG-CL-021 / 1`, `CFG-CL-033 / 1`,
//! `CFG-CL-034 / 2`, `CFG-CL-038 / 1`, `CFG-CL-039 / 1`, `CFG-CL-040 / 1`,
//! `FR-CL-015 / 1`, `FR-CL-017 / 1`, `FR-CL-027 / 1`, `INT-NET-071 / 1`,
//! `NFR-SEC-008 / 1`.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use serde::Deserialize;

use crate::raft::NodeId;
use crate::sealing::RecipientPublicKey;
use crate::server_transport::{Peer, PeerDirectory, PeerDirectoryError};
use crate::signing::MessageVerifyingKey;

/// Fallible manifest operation result.
type Result<T> = core::result::Result<T, ManifestError>;

/// Maximum accepted label length in UTF-8 bytes.
pub const MAX_MANIFEST_LABEL_LEN: usize = 4096;

/// Sender identifier length in bytes.
const SENDER_ID_LEN: usize = 16;

/// Hyphenated sender identifier text length.
const SENDER_ID_TEXT_LEN: usize = 36;

/// Compact sender identifier text length.
const SENDER_ID_COMPACT_LEN: usize = 32;

/// Public key length in bytes.
const PUBLIC_KEY_LEN: usize = 32;

/// Positions of the hyphens in a hyphenated sender identifier.
const SENDER_ID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

/// One trusted server entry from the static manifest.
pub struct ManifestEntry {
    /// Unique 16-byte sender identifier.
    pub sender_id: [u8; SENDER_ID_LEN],
    /// Unique operator-assigned label.
    pub label: String,
    /// Canonical IP address and non-zero port.
    pub address: SocketAddr,
    /// X25519 public key used to seal messages to this server.
    pub x25519_public_key: [u8; PUBLIC_KEY_LEN],
    /// Ed25519 public key used to verify this server's signatures.
    pub ed25519_public_key: [u8; PUBLIC_KEY_LEN],
}

impl ManifestEntry {
    /// Returns the Raft node identifier derived from the first eight sender-id bytes.
    pub fn node_id(&self) -> NodeId {
        let [first, second, third, fourth, fifth, sixth, seventh, eighth, ..] = self.sender_id;
        NodeId::from_be_bytes([first, second, third, fourth, fifth, sixth, seventh, eighth])
    }
}

/// Validated static manifest of the dedicated server and storage cluster.
pub struct StaticManifest {
    /// The dedicated registration server.
    pub dedicated: ManifestEntry,
    /// The fixed storage-cluster nodes.
    pub storage: Vec<ManifestEntry>,
}

impl StaticManifest {
    /// Parses and validates a manifest from TOML text.
    pub fn parse(text: &str) -> Result<Self> {
        let file = toml::from_str::<ManifestFile>(text).map_err(|_| ManifestError::Toml)?;
        Self::validate(file)
    }

    /// Reads, decodes, parses, and validates a manifest file.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(ManifestError::Read)?;
        let text = String::from_utf8(bytes).map_err(|_| ManifestError::Utf8)?;
        Self::parse(&text)
    }

    /// Returns the storage entry with the exact local label.
    pub fn local(&self, label: &str) -> Result<&ManifestEntry> {
        self.storage
            .iter()
            .find(|entry| entry.label == label)
            .ok_or(ManifestError::LocalNotFound)
    }

    /// Returns the fixed Raft voter set as node ids and advertised addresses.
    pub fn voting_members(&self) -> Result<BTreeMap<NodeId, String>> {
        Ok(self
            .storage
            .iter()
            .map(|entry| (entry.node_id(), entry.address.to_string()))
            .collect())
    }

    /// Builds the trusted peer directory from the storage entries.
    pub fn peer_directory(&self) -> Result<PeerDirectory> {
        let peers = self
            .storage
            .iter()
            .map(|entry| {
                Peer::new(
                    entry.node_id(),
                    entry.sender_id,
                    entry.address,
                    &entry.x25519_public_key,
                    entry.ed25519_public_key,
                )
                .map_err(|_| ManifestError::InvalidPublicKey)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PeerDirectory::new(peers)?)
    }

    fn validate(file: ManifestFile) -> Result<Self> {
        if file.storage.is_empty() {
            return Err(ManifestError::EmptyStorage);
        }
        let dedicated = file.dedicated.into_entry()?;
        let storage = file
            .storage
            .into_iter()
            .map(NodeEntry::into_entry)
            .collect::<Result<Vec<_>>>()?;
        check_unique(&dedicated, &storage)?;
        Ok(Self { dedicated, storage })
    }
}

/// Failure modes when loading or validating a static manifest.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The manifest file could not be read.
    #[error("the manifest file could not be read")]
    Read(#[source] std::io::Error),
    /// The manifest bytes are not valid UTF-8.
    #[error("the manifest is not valid UTF-8")]
    Utf8,
    /// The manifest text is not valid TOML.
    #[error("the manifest is not valid TOML")]
    Toml,
    /// The manifest has no storage entries.
    #[error("the manifest has no storage entries")]
    EmptyStorage,
    /// A label is empty or too long.
    #[error("the manifest label is empty or too long")]
    InvalidLabel,
    /// A sender identifier is not a 16-byte UUID.
    #[error("the manifest sender id is not a 16-byte UUID")]
    InvalidSenderId,
    /// A public key is not 32 bytes of hex or not a valid curve point.
    #[error("the manifest public key is not 32 bytes of hex")]
    InvalidPublicKey,
    /// A host is not an IP literal.
    #[error("the manifest host is not a canonical IP literal")]
    InvalidHost,
    /// A port is zero.
    #[error("the manifest port is zero")]
    InvalidPort,
    /// Two entries share a label.
    #[error("duplicate manifest label")]
    DuplicateLabel,
    /// Two entries share a sender identifier.
    #[error("duplicate manifest sender id")]
    DuplicateSender,
    /// Two entries share an address.
    #[error("duplicate manifest address")]
    DuplicateAddress,
    /// Two storage entries map to the same Raft node id.
    #[error("two manifest entries map to the same Raft node id")]
    NodeIdCollision,
    /// The local label does not identify a storage entry.
    #[error("the local label is not a storage entry")]
    LocalNotFound,
    /// The trusted peer directory rejected the storage entries.
    #[error(transparent)]
    PeerDirectory(#[from] PeerDirectoryError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    dedicated: NodeEntry,
    storage: Vec<NodeEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeEntry {
    sender_id: String,
    label: String,
    host: String,
    port: u16,
    x25519_public_key: String,
    ed25519_public_key: String,
}

impl NodeEntry {
    fn into_entry(self) -> Result<ManifestEntry> {
        if self.label.is_empty() || self.label.len() > MAX_MANIFEST_LABEL_LEN {
            return Err(ManifestError::InvalidLabel);
        }
        let sender_id = parse_sender_id(&self.sender_id)?;
        if self.port == 0 {
            return Err(ManifestError::InvalidPort);
        }
        let address = SocketAddr::new(
            self.host
                .parse::<IpAddr>()
                .map_err(|_| ManifestError::InvalidHost)?,
            self.port,
        );
        let x25519_public_key = parse_public_key(&self.x25519_public_key)?;
        RecipientPublicKey::from_bytes(&x25519_public_key)
            .map_err(|_| ManifestError::InvalidPublicKey)?;
        let ed25519_public_key = parse_public_key(&self.ed25519_public_key)?;
        MessageVerifyingKey::from_bytes(ed25519_public_key)
            .map_err(|_| ManifestError::InvalidPublicKey)?;
        Ok(ManifestEntry {
            sender_id,
            label: self.label,
            address,
            x25519_public_key,
            ed25519_public_key,
        })
    }
}

fn check_unique(dedicated: &ManifestEntry, storage: &[ManifestEntry]) -> Result<()> {
    let mut labels = BTreeSet::new();
    let mut sender_ids = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    for entry in core::iter::once(dedicated).chain(storage) {
        if !labels.insert(entry.label.as_str()) {
            return Err(ManifestError::DuplicateLabel);
        }
        if !sender_ids.insert(entry.sender_id) {
            return Err(ManifestError::DuplicateSender);
        }
        if !addresses.insert(entry.address) {
            return Err(ManifestError::DuplicateAddress);
        }
    }
    let mut node_ids = BTreeSet::new();
    for entry in storage {
        if !node_ids.insert(entry.node_id()) {
            return Err(ManifestError::NodeIdCollision);
        }
    }
    Ok(())
}

fn parse_sender_id(text: &str) -> Result<[u8; SENDER_ID_LEN]> {
    let bytes = text.as_bytes();
    let compact = if bytes.len() == SENDER_ID_TEXT_LEN {
        compact_sender_id(bytes).ok_or(ManifestError::InvalidSenderId)?
    } else if bytes.len() == SENDER_ID_COMPACT_LEN {
        let mut compact = [0u8; SENDER_ID_COMPACT_LEN];
        compact.copy_from_slice(bytes);
        compact
    } else {
        return Err(ManifestError::InvalidSenderId);
    };
    decode_hex::<SENDER_ID_LEN>(&compact).ok_or(ManifestError::InvalidSenderId)
}

fn compact_sender_id(bytes: &[u8]) -> Option<[u8; SENDER_ID_COMPACT_LEN]> {
    let mut compact = [0u8; SENDER_ID_COMPACT_LEN];
    let mut next = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if SENDER_ID_HYPHENS.contains(&index) {
            if *byte != b'-' {
                return None;
            }
        } else {
            compact[next] = *byte;
            next += 1;
        }
    }
    Some(compact)
}

fn parse_public_key(text: &str) -> Result<[u8; PUBLIC_KEY_LEN]> {
    decode_hex::<PUBLIC_KEY_LEN>(text.as_bytes()).ok_or(ManifestError::InvalidPublicKey)
}

fn decode_hex<const N: usize>(text: &[u8]) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut decoded = [0u8; N];
    for (index, pair) in text.as_chunks::<2>().0.iter().enumerate() {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        decoded[index] = (high << 4) | low;
    }
    Some(decoded)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
