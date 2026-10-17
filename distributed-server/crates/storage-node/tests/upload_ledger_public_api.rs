//! Integration tests exercising only the public `storage_node` upload ledger API.
//!
//! Covers `INT-NET-011 / 1`: an upload identifier is a 16-byte value transmitted
//! as raw bytes and rendered as 32 lowercase hex characters.

use storage_node::{
    ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
    CONTENT_HASH_LEN, UPLOAD_ID_LEN,
};

fn upload_id(seed: u8) -> UploadId {
    UploadId::from_bytes([seed; UPLOAD_ID_LEN])
}

fn content_hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; CONTENT_HASH_LEN])
}

#[test]
fn upload_id_carries_the_sixteen_wire_bytes() {
    let id = UploadId::from_bytes([0xab; UPLOAD_ID_LEN]);

    assert_eq!(id.as_bytes(), &[0xab; UPLOAD_ID_LEN]);
    assert_eq!(id.to_string(), "ab".repeat(UPLOAD_ID_LEN));
    assert_eq!(id.to_string().len(), UPLOAD_ID_LEN * 2);
}

#[test]
fn upload_record_new_trims_and_rejects_blank_fields() {
    let hash = content_hash(0x2a);
    let record = UploadRecord::new(
        upload_id(1),
        "  alice  ",
        "  photo.jpg  ",
        4_096,
        hash,
        1_700_000_000_000,
    )
    .expect("record with non-blank fields is valid");

    assert_eq!(record.upload_id, upload_id(1));
    assert_eq!(record.login, "alice");
    assert_eq!(record.original_name, "photo.jpg");
    assert_eq!(record.size_bytes, 4_096);
    assert_eq!(record.content_hash, hash);
    assert_eq!(record.accepted_at_ms, 1_700_000_000_000);

    assert_eq!(
        UploadRecord::new(upload_id(1), "   ", "photo.jpg", 1, hash, 0),
        Err(MetadataError::EmptyLogin)
    );
    assert_eq!(
        UploadRecord::new(upload_id(1), "alice", "   ", 1, hash, 0),
        Err(MetadataError::EmptyOriginalName)
    );
}

#[test]
fn ledger_records_first_upload_and_keeps_it_on_retry() {
    let mut ledger = UploadLedger::new();
    let first_hash = content_hash(0x11);
    let first = UploadRecord::new(upload_id(1), "alice", "a.txt", 10, first_hash, 1_000)
        .expect("first record is valid");

    assert_eq!(ledger.record(first), RecordOutcome::Recorded);
    assert_eq!(ledger.len(), 1);

    let retry_hash = content_hash(0x22);
    let retry = UploadRecord::new(upload_id(1), "bob", "b.txt", 99, retry_hash, 2_000)
        .expect("retry record is valid");

    assert_eq!(ledger.record(retry), RecordOutcome::AlreadyPresent);
    assert_eq!(ledger.len(), 1);

    let stored = ledger.get(&upload_id(1)).expect("first record is stored");
    assert_eq!(stored.login, "alice");
    assert_eq!(stored.original_name, "a.txt");
    assert_eq!(stored.size_bytes, 10);
    assert_eq!(stored.content_hash, first_hash);
    assert_eq!(stored.accepted_at_ms, 1_000);
}

#[test]
fn ledger_get_unknown_id_returns_none() {
    let mut ledger = UploadLedger::new();
    let unknown = UploadId::from_bytes([0xff; UPLOAD_ID_LEN]);

    assert!(ledger.is_empty());
    assert_eq!(ledger.get(&unknown), None);

    let record = UploadRecord::new(
        upload_id(1),
        "alice",
        "a.txt",
        10,
        content_hash(0x33),
        1_000,
    )
    .expect("record is valid");
    ledger.record(record);

    assert!(!ledger.is_empty());
    assert_eq!(ledger.get(&unknown), None);
}
