//! Integration tests exercising only the public `storage_node` upload ledger API.

use storage_node::{
    ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
    CONTENT_HASH_LEN, MAX_UPLOAD_ID_LEN,
};

const UPLOAD_ID: &str = "upload-1";

fn upload_id() -> UploadId {
    UploadId::parse(UPLOAD_ID).expect("test upload id uses allowed characters")
}

fn content_hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; CONTENT_HASH_LEN])
}

#[test]
fn upload_id_round_trips_through_display_and_from_str() {
    let id = UploadId::parse("upload-01_A").expect("id uses allowed characters");

    assert_eq!(id.as_str(), "upload-01_A");
    assert_eq!(id.to_string(), "upload-01_A");
    assert_eq!("upload-01_A".parse::<UploadId>(), Ok(id));
}

#[test]
fn upload_id_parse_rejects_invalid_values() {
    assert_eq!(UploadId::parse(""), Err(MetadataError::EmptyUploadId));

    let over_long = "a".repeat(MAX_UPLOAD_ID_LEN + 1);
    assert_eq!(
        UploadId::parse(&over_long),
        Err(MetadataError::UploadIdTooLong {
            len: MAX_UPLOAD_ID_LEN + 1,
            max: MAX_UPLOAD_ID_LEN,
        })
    );

    assert_eq!(
        UploadId::parse("bad id"),
        Err(MetadataError::InvalidUploadIdCharacter { character: ' ' })
    );
}

#[test]
fn content_hash_hex_round_trips() {
    let hash = ContentHash::from_bytes(std::array::from_fn(|index| index as u8));
    let hex = hash.to_hex();

    assert_eq!(hex.len(), CONTENT_HASH_LEN * 2);
    assert_eq!(hex, hex.to_ascii_lowercase());
    assert!(hex.chars().all(|character| character.is_ascii_hexdigit()));
    assert_eq!(ContentHash::parse_hex(&hex), Ok(hash));
    assert_eq!(hash.as_bytes(), &std::array::from_fn(|index| index as u8));
}

#[test]
fn content_hash_parse_hex_rejects_bad_input() {
    assert_eq!(
        ContentHash::parse_hex("abc"),
        Err(MetadataError::InvalidContentHashLength { len: 3 })
    );

    let non_hex = "z".repeat(CONTENT_HASH_LEN * 2);
    assert_eq!(
        ContentHash::parse_hex(&non_hex),
        Err(MetadataError::InvalidContentHashHex { character: 'z' })
    );
}

#[test]
fn upload_record_new_trims_and_rejects_blank_fields() {
    let hash = content_hash(0x2a);
    let record = UploadRecord::new(
        upload_id(),
        "  alice  ",
        "  photo.jpg  ",
        4_096,
        hash,
        1_700_000_000_000,
    )
    .expect("record with non-blank fields is valid");

    assert_eq!(record.upload_id(), &upload_id());
    assert_eq!(record.login(), "alice");
    assert_eq!(record.original_name(), "photo.jpg");
    assert_eq!(record.size_bytes(), 4_096);
    assert_eq!(record.content_hash(), hash);
    assert_eq!(record.accepted_at_ms(), 1_700_000_000_000);

    assert_eq!(
        UploadRecord::new(upload_id(), "   ", "photo.jpg", 1, hash, 0),
        Err(MetadataError::EmptyLogin)
    );
    assert_eq!(
        UploadRecord::new(upload_id(), "alice", "   ", 1, hash, 0),
        Err(MetadataError::EmptyOriginalName)
    );
}

#[test]
fn ledger_records_first_upload_and_keeps_it_on_retry() {
    let mut ledger = UploadLedger::new();
    let first_hash = content_hash(0x11);
    let first = UploadRecord::new(upload_id(), "alice", "a.txt", 10, first_hash, 1_000)
        .expect("first record is valid");

    assert_eq!(ledger.record(first), RecordOutcome::Recorded);
    assert_eq!(ledger.len(), 1);

    let retry_hash = content_hash(0x22);
    let retry = UploadRecord::new(upload_id(), "bob", "b.txt", 99, retry_hash, 2_000)
        .expect("retry record is valid");

    assert_eq!(ledger.record(retry), RecordOutcome::AlreadyPresent);
    assert_eq!(ledger.len(), 1);

    let stored = ledger.get(&upload_id()).expect("first record is stored");
    assert_eq!(stored.login(), "alice");
    assert_eq!(stored.original_name(), "a.txt");
    assert_eq!(stored.size_bytes(), 10);
    assert_eq!(stored.content_hash(), first_hash);
    assert_eq!(stored.accepted_at_ms(), 1_000);
}

#[test]
fn ledger_get_unknown_id_returns_none() {
    let mut ledger = UploadLedger::new();
    let unknown = UploadId::parse("unknown").expect("id uses allowed characters");

    assert!(ledger.is_empty());
    assert_eq!(ledger.get(&unknown), None);

    let record = UploadRecord::new(upload_id(), "alice", "a.txt", 10, content_hash(0x33), 1_000)
        .expect("record is valid");
    ledger.record(record);

    assert!(!ledger.is_empty());
    assert_eq!(ledger.get(&unknown), None);
}
