//! Public API integration tests for the static TOML manifest.
//!
//! Covers `CFG-CL-001 / 1`, `CFG-CL-002 / 1`, `CFG-CL-003 / 1`,
//! `CFG-CL-004 / 1`, `CFG-CL-005 / 1`, and `CFG-CL-021 / 1`: every storage
//! node and the dedicated server have a unique label, an address, and X25519
//! and Ed25519 open keys in the same operator-provided manifest. Covers
//! `CFG-CL-034 / 2`: the manifest is TOML 1.0 encoded as UTF-8. Covers
//! `CFG-CL-038 / 1`: required fields are checked and unknown or duplicate
//! fields are rejected. Covers `NFR-SEC-008 / 1`: the manifest carries only
//! open keys. Covers `FR-CL-015 / 1`, `FR-CL-017 / 1`, and `FR-CL-027 / 1`:
//! Raft membership is the fixed set of storage nodes from the manifest, and
//! `INT-NET-071 / 1`: each node's `sender_id` is a unique UUID taken from the manifest.

use std::fs;

use storage_node::{
    ManifestError, MessageSigningKey, RecipientPrivateKey, StaticManifest, MAX_MANIFEST_LABEL_LEN,
};

const DEDICATED_SENDER_ID: &str = "33333333-3333-4333-8333-333333333333";
const DEDICATED_LABEL: &str = "dedicated";
const DEDICATED_HOST: &str = "127.0.0.3";
const DEDICATED_PORT: u16 = 7000;
const DEDICATED_ADDRESS: &str = "127.0.0.3:7000";
const DEDICATED_X25519_SEED: u8 = 0xC1;
const DEDICATED_ED25519_SEED: u8 = 0xC2;

const STORAGE_A_SENDER_ID: &str = "11111111-1111-4111-8111-111111111111";
const STORAGE_A_LABEL: &str = "storage-a";
const STORAGE_A_HOST: &str = "127.0.0.1";
const STORAGE_A_PORT: u16 = 7001;
const STORAGE_A_ADDRESS: &str = "127.0.0.1:7001";
const STORAGE_A_X25519_SEED: u8 = 0xA1;
const STORAGE_A_ED25519_SEED: u8 = 0xA2;

const STORAGE_B_SENDER_ID: &str = "22222222-2222-4222-8222-222222222222";
const STORAGE_B_LABEL: &str = "storage-b";
const STORAGE_B_HOST: &str = "127.0.0.2";
const STORAGE_B_PORT: u16 = 7002;
const STORAGE_B_ADDRESS: &str = "127.0.0.2:7002";
const STORAGE_B_X25519_SEED: u8 = 0xB1;
const STORAGE_B_ED25519_SEED: u8 = 0xB2;

const COLLISION_SENDER_ID_A: &str = "00112233-4455-4677-8899-aabbccddeeff";
const COLLISION_SENDER_ID_B: &str = "00112233-4455-4677-8899-000000000000";

const INVALID_SENDER_ID: &str = "not-a-uuid";
const DNS_HOST: &str = "storage-a.example";
const LOCAL_LABEL: &str = "unknown-storage";

struct EntrySpec {
    sender_id: String,
    label: String,
    host: String,
    port: u32,
    x25519_public_key: String,
    ed25519_public_key: String,
}

impl EntrySpec {
    fn new(
        sender_id: &str,
        label: &str,
        host: &str,
        port: u16,
        x25519_seed: u8,
        ed25519_seed: u8,
    ) -> Self {
        Self {
            sender_id: sender_id.to_owned(),
            label: label.to_owned(),
            host: host.to_owned(),
            port: u32::from(port),
            x25519_public_key: key_hex(&recipient_public_key_bytes(x25519_seed)),
            ed25519_public_key: key_hex(&verifying_key_bytes(ed25519_seed)),
        }
    }
}

fn key_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn recipient_public_key_bytes(seed: u8) -> [u8; 32] {
    RecipientPrivateKey::from_bytes(&[seed; 32])
        .expect("the fixed X25519 seed builds a valid private key")
        .public_key()
        .as_bytes()
}

fn verifying_key_bytes(seed: u8) -> [u8; 32] {
    MessageSigningKey::from_bytes([seed; 32])
        .verifying_key()
        .as_bytes()
}

fn dedicated_spec() -> EntrySpec {
    EntrySpec::new(
        DEDICATED_SENDER_ID,
        DEDICATED_LABEL,
        DEDICATED_HOST,
        DEDICATED_PORT,
        DEDICATED_X25519_SEED,
        DEDICATED_ED25519_SEED,
    )
}

fn storage_a_spec() -> EntrySpec {
    EntrySpec::new(
        STORAGE_A_SENDER_ID,
        STORAGE_A_LABEL,
        STORAGE_A_HOST,
        STORAGE_A_PORT,
        STORAGE_A_X25519_SEED,
        STORAGE_A_ED25519_SEED,
    )
}

fn storage_b_spec() -> EntrySpec {
    EntrySpec::new(
        STORAGE_B_SENDER_ID,
        STORAGE_B_LABEL,
        STORAGE_B_HOST,
        STORAGE_B_PORT,
        STORAGE_B_X25519_SEED,
        STORAGE_B_ED25519_SEED,
    )
}

fn render_entry(header: &str, spec: &EntrySpec) -> String {
    format!(
        "{header}\nsender_id = \"{}\"\nlabel = \"{}\"\nhost = \"{}\"\nport = {}\nx25519_public_key = \"{}\"\ned25519_public_key = \"{}\"\n",
        spec.sender_id,
        spec.label,
        spec.host,
        spec.port,
        spec.x25519_public_key,
        spec.ed25519_public_key,
    )
}

fn manifest_text(dedicated: Option<&EntrySpec>, storage: &[EntrySpec]) -> String {
    let mut text = String::new();
    if let Some(spec) = dedicated {
        text.push_str(&render_entry("[dedicated]", spec));
        text.push('\n');
    }
    for spec in storage {
        text.push_str(&render_entry("[[storage]]", spec));
        text.push('\n');
    }
    text
}

fn valid_manifest() -> String {
    manifest_text(
        Some(&dedicated_spec()),
        &[storage_a_spec(), storage_b_spec()],
    )
}

fn parse_error(source: &str) -> ManifestError {
    StaticManifest::parse(source)
        .err()
        .expect("the invalid manifest is rejected")
}

#[test]
fn parses_a_full_manifest_and_projects_local_peer_and_voter_views() {
    let manifest = StaticManifest::parse(&valid_manifest()).expect("the full manifest parses");

    let dedicated = &manifest.dedicated;
    assert_eq!(
        dedicated.sender_id,
        [
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x43, 0x33, 0x83, 0x33, 0x33, 0x33, 0x33, 0x33,
            0x33, 0x33,
        ]
    );
    assert_eq!(dedicated.label, DEDICATED_LABEL);
    assert_eq!(dedicated.node_id(), 0x3333_3333_3333_4333);
    assert_eq!(dedicated.address.to_string(), DEDICATED_ADDRESS);
    assert_eq!(
        dedicated.x25519_public_key,
        recipient_public_key_bytes(DEDICATED_X25519_SEED)
    );
    assert_eq!(
        dedicated.ed25519_public_key,
        verifying_key_bytes(DEDICATED_ED25519_SEED)
    );

    assert_eq!(manifest.storage.len(), 2);
    let storage_a = &manifest.storage[0];
    assert_eq!(
        storage_a.sender_id,
        [
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x41, 0x11, 0x81, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11,
        ]
    );
    assert_eq!(storage_a.label, STORAGE_A_LABEL);
    assert_eq!(storage_a.node_id(), 0x1111_1111_1111_4111);
    assert_eq!(storage_a.address.to_string(), STORAGE_A_ADDRESS);
    assert_eq!(
        storage_a.x25519_public_key,
        recipient_public_key_bytes(STORAGE_A_X25519_SEED)
    );
    assert_eq!(
        storage_a.ed25519_public_key,
        verifying_key_bytes(STORAGE_A_ED25519_SEED)
    );

    let storage_b = &manifest.storage[1];
    assert_eq!(
        storage_b.sender_id,
        [
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x42, 0x22, 0x82, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22,
        ]
    );
    assert_eq!(storage_b.label, STORAGE_B_LABEL);
    assert_eq!(storage_b.node_id(), 0x2222_2222_2222_4222);
    assert_eq!(storage_b.address.to_string(), STORAGE_B_ADDRESS);
    assert_eq!(
        storage_b.x25519_public_key,
        recipient_public_key_bytes(STORAGE_B_X25519_SEED)
    );
    assert_eq!(
        storage_b.ed25519_public_key,
        verifying_key_bytes(STORAGE_B_ED25519_SEED)
    );

    let local = manifest
        .local(STORAGE_A_LABEL)
        .expect("the storage label resolves the local entry");
    assert!(core::ptr::eq(local, storage_a));

    let voters = manifest
        .voting_members()
        .expect("the storage entries form a voter set");
    assert_eq!(voters.len(), 2);
    assert_eq!(
        voters.get(&storage_a.node_id()).map(String::as_str),
        Some(STORAGE_A_ADDRESS)
    );
    assert_eq!(
        voters.get(&storage_b.node_id()).map(String::as_str),
        Some(STORAGE_B_ADDRESS)
    );
    assert!(
        !voters.contains_key(&dedicated.node_id()),
        "the dedicated server is not a Raft voter"
    );

    let directory = manifest
        .peer_directory()
        .expect("the manifest builds a trusted peer directory");
    for entry in [storage_a, storage_b] {
        let peer = directory
            .get(entry.node_id())
            .expect("each storage node id resolves in the peer directory");
        assert_eq!(peer.node_id, entry.node_id());
        assert_eq!(peer.sender_id, entry.sender_id);
        assert_eq!(peer.address, entry.address);
        assert_eq!(peer.x25519_public_key.as_bytes(), entry.x25519_public_key);
        assert_eq!(peer.ed25519_public_key.as_bytes(), entry.ed25519_public_key);
    }
}

#[test]
fn load_reads_utf8_toml() {
    let source = format!("# статический манифест — UTF-8\n{}", valid_manifest());
    let file = tempfile::NamedTempFile::new().expect("the temporary manifest file is created");
    fs::write(file.path(), source.as_bytes()).expect("the manifest text is written");

    let manifest = StaticManifest::load(file.path()).expect("the UTF-8 TOML manifest loads");

    assert_eq!(manifest.dedicated.label, DEDICATED_LABEL);
    assert_eq!(manifest.dedicated.address.to_string(), DEDICATED_ADDRESS);
    assert_eq!(
        manifest.dedicated.x25519_public_key,
        recipient_public_key_bytes(DEDICATED_X25519_SEED)
    );
    assert_eq!(manifest.storage.len(), 2);
    assert_eq!(
        manifest
            .local(STORAGE_B_LABEL)
            .expect("the loaded storage entry is present")
            .address
            .to_string(),
        STORAGE_B_ADDRESS
    );

    let invalid_utf8 =
        tempfile::NamedTempFile::new().expect("the invalid manifest file is created");
    fs::write(invalid_utf8.path(), [0xFF, 0xFE, 0x00]).expect("the invalid bytes are written");
    let invalid_utf8_error = StaticManifest::load(invalid_utf8.path())
        .err()
        .expect("a manifest that is not UTF-8 is rejected");
    assert!(matches!(invalid_utf8_error, ManifestError::Utf8));
}

#[test]
fn rejects_missing_unknown_duplicate_and_malformed_toml() {
    let missing_dedicated = manifest_text(None, &[storage_a_spec(), storage_b_spec()]);
    assert!(matches!(
        parse_error(&missing_dedicated),
        ManifestError::Toml
    ));

    let missing_storage = manifest_text(Some(&dedicated_spec()), &[]);
    assert!(matches!(parse_error(&missing_storage), ManifestError::Toml));

    let empty_storage = format!(
        "storage = []\n{}",
        render_entry("[dedicated]", &dedicated_spec())
    );
    assert!(matches!(
        parse_error(&empty_storage),
        ManifestError::EmptyStorage
    ));

    let missing_field = valid_manifest().replace("port = 7000\n", "");
    assert!(matches!(parse_error(&missing_field), ManifestError::Toml));

    let unknown_field = format!("{}unknown = \"value\"\n", valid_manifest());
    assert!(matches!(parse_error(&unknown_field), ManifestError::Toml));

    let top_level_unknown = format!("unknown = \"value\"\n{}", valid_manifest());
    assert!(matches!(
        parse_error(&top_level_unknown),
        ManifestError::Toml
    ));

    let duplicate_key = valid_manifest().replace(
        "label = \"dedicated\"\n",
        "label = \"dedicated\"\nlabel = \"dedicated\"\n",
    );
    assert!(matches!(parse_error(&duplicate_key), ManifestError::Toml));

    let syntax_error = format!("{}invalid\n", valid_manifest());
    assert!(matches!(parse_error(&syntax_error), ManifestError::Toml));
}

#[test]
fn rejects_duplicate_labels_senders_addresses_and_node_id_collisions() {
    let mut duplicate_label = storage_b_spec();
    duplicate_label.label = STORAGE_A_LABEL.to_owned();
    let source = manifest_text(
        Some(&dedicated_spec()),
        &[storage_a_spec(), duplicate_label],
    );
    assert!(matches!(
        parse_error(&source),
        ManifestError::DuplicateLabel
    ));

    let mut duplicate_sender = storage_b_spec();
    duplicate_sender.sender_id = STORAGE_A_SENDER_ID.to_owned();
    let source = manifest_text(
        Some(&dedicated_spec()),
        &[storage_a_spec(), duplicate_sender],
    );
    assert!(matches!(
        parse_error(&source),
        ManifestError::DuplicateSender
    ));

    let mut duplicate_address = storage_b_spec();
    duplicate_address.host = STORAGE_A_HOST.to_owned();
    duplicate_address.port = u32::from(STORAGE_A_PORT);
    let source = manifest_text(
        Some(&dedicated_spec()),
        &[storage_a_spec(), duplicate_address],
    );
    assert!(matches!(
        parse_error(&source),
        ManifestError::DuplicateAddress
    ));

    let mut first = storage_a_spec();
    first.sender_id = COLLISION_SENDER_ID_A.to_owned();
    let mut second = storage_b_spec();
    second.sender_id = COLLISION_SENDER_ID_B.to_owned();
    let source = manifest_text(Some(&dedicated_spec()), &[first, second]);
    assert!(matches!(
        parse_error(&source),
        ManifestError::NodeIdCollision
    ));
}

#[test]
fn rejects_invalid_identifiers_keys_hosts_and_labels() {
    let mut invalid_sender = storage_a_spec();
    invalid_sender.sender_id = INVALID_SENDER_ID.to_owned();
    let source = manifest_text(Some(&dedicated_spec()), &[invalid_sender]);
    assert!(matches!(
        parse_error(&source),
        ManifestError::InvalidSenderId
    ));

    let mut short_key = storage_a_spec();
    short_key.x25519_public_key = "a".repeat(63);
    let source = manifest_text(Some(&dedicated_spec()), &[short_key]);
    assert!(matches!(
        parse_error(&source),
        ManifestError::InvalidPublicKey
    ));

    let mut non_hex_key = storage_a_spec();
    non_hex_key.x25519_public_key = "z".repeat(64);
    let source = manifest_text(Some(&dedicated_spec()), &[non_hex_key]);
    assert!(matches!(
        parse_error(&source),
        ManifestError::InvalidPublicKey
    ));

    let mut invalid_ed25519 = storage_a_spec();
    invalid_ed25519.ed25519_public_key = format!("02{}", "00".repeat(31));
    let source = manifest_text(Some(&dedicated_spec()), &[invalid_ed25519]);
    assert!(matches!(
        parse_error(&source),
        ManifestError::InvalidPublicKey
    ));

    let mut dns_host = storage_a_spec();
    dns_host.host = DNS_HOST.to_owned();
    let source = manifest_text(Some(&dedicated_spec()), &[dns_host]);
    assert!(matches!(parse_error(&source), ManifestError::InvalidHost));

    let mut zone_host = storage_a_spec();
    zone_host.host = "fe80::1%eth0".to_owned();
    let source = manifest_text(Some(&dedicated_spec()), &[zone_host]);
    assert!(matches!(parse_error(&source), ManifestError::InvalidHost));

    let mut overflow_port = storage_a_spec();
    overflow_port.port = 70_000;
    let source = manifest_text(Some(&dedicated_spec()), &[overflow_port]);
    assert!(matches!(parse_error(&source), ManifestError::Toml));

    let negative_port = valid_manifest().replace("port = 7001", "port = -1");
    assert!(matches!(parse_error(&negative_port), ManifestError::Toml));

    let mut zero_port = storage_a_spec();
    zero_port.port = 0;
    let source = manifest_text(Some(&dedicated_spec()), &[zero_port]);
    assert!(matches!(parse_error(&source), ManifestError::InvalidPort));

    let mut empty_label = storage_a_spec();
    empty_label.label = String::new();
    let source = manifest_text(Some(&dedicated_spec()), &[empty_label]);
    assert!(matches!(parse_error(&source), ManifestError::InvalidLabel));

    let mut oversized_label = storage_a_spec();
    oversized_label.label = "a".repeat(MAX_MANIFEST_LABEL_LEN + 1);
    let source = manifest_text(Some(&dedicated_spec()), &[oversized_label]);
    assert!(matches!(parse_error(&source), ManifestError::InvalidLabel));
}

#[test]
fn local_lookup_requires_a_storage_label() {
    let manifest = StaticManifest::parse(&valid_manifest()).expect("the full manifest parses");

    let dedicated_error = manifest
        .local(DEDICATED_LABEL)
        .err()
        .expect("the dedicated label is not a storage entry");
    assert!(matches!(dedicated_error, ManifestError::LocalNotFound));

    let unknown_error = manifest
        .local(LOCAL_LABEL)
        .err()
        .expect("an unknown label is not a storage entry");
    assert!(matches!(unknown_error, ManifestError::LocalNotFound));
}
