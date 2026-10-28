//! Integration tests exercising only the public `storage_node` catalog codecs.
//!
//! Covers `FR-CAT-012 / 3`: catalog entries carry the upload id, the original
//! source name, the capture time, the original and compressed sizes, and the
//! SHA-256 of the original bytes. Covers `FR-CAT-013 / 2`: pages are encoded as
//! CBOR `[snapshot_token, next_cursor, entries]` with a null cursor on the last
//! page. Covers `FR-CAT-015 / 1`: requests are CBOR `[snapshot_token, cursor]`
//! where both tokens are either null or 16-byte byte strings. Supports
//! `INT-NET-053 / 2` and `FR-NODE-008 / 1`.

use std::collections::BTreeSet;

use storage_node::decode_request;
use storage_node::encode_page;
use storage_node::CatalogEntry;
use storage_node::CatalogError;
use storage_node::CatalogPage;
use storage_node::CommittedVersion;
use storage_node::ContentHash;
use storage_node::UploadId;
use storage_node::UploadRecord;
use storage_node::CATALOG_TOKEN_LEN;
use storage_node::CONTENT_HASH_LEN;
use storage_node::UPLOAD_ID_LEN;

/// Source-name byte limit used by the mapping tests.
const MAX_SOURCE_NAME_LEN: usize = 64;

fn upload_id(seed: u8) -> UploadId {
    UploadId::from_bytes([seed; UPLOAD_ID_LEN])
}

fn content_hash(seed: u8) -> ContentHash {
    ContentHash::from_bytes([seed; CONTENT_HASH_LEN])
}

fn token(seed: u8) -> [u8; CATALOG_TOKEN_LEN] {
    [seed; CATALOG_TOKEN_LEN]
}

fn committed_version(record: UploadRecord) -> CommittedVersion {
    CommittedVersion {
        record,
        replicas: BTreeSet::from([7, 8]),
    }
}

#[test]
fn encodes_a_last_page_as_the_positional_golden_bytes() {
    let entry = CatalogEntry::new(
        upload_id(0x01),
        "a",
        0,
        0,
        0,
        content_hash(0x02),
        MAX_SOURCE_NAME_LEN,
    )
    .expect("the source name is within the limit");
    let page = CatalogPage {
        snapshot_token: token(0x03),
        next_cursor: None,
        entries: vec![entry],
    };

    let encoded = encode_page(&page).expect("the page encodes");

    let mut expected = vec![0x83, 0x50];
    expected.extend(token(0x03));
    expected.extend([0xf6, 0x81, 0x86, 0x50]);
    expected.extend([0x01; UPLOAD_ID_LEN]);
    expected.extend([0x61, 0x61, 0x00, 0x00, 0x00, 0x58, 0x20]);
    expected.extend([0x02; CONTENT_HASH_LEN]);
    assert_eq!(encoded, expected);
}

#[test]
fn encodes_a_continuation_page_cursor() {
    let page = CatalogPage {
        snapshot_token: token(0x03),
        next_cursor: Some(token(0x04)),
        entries: Vec::new(),
    };

    let encoded = encode_page(&page).expect("the page encodes");

    let mut expected = vec![0x83, 0x50];
    expected.extend(token(0x03));
    expected.push(0x50);
    expected.extend(token(0x04));
    expected.push(0x80);
    assert_eq!(encoded, expected);
}

#[test]
fn maps_a_committed_version_to_its_catalog_entry() {
    let record = UploadRecord::new(
        upload_id(0x05),
        "alice",
        "  photo.jpg  ",
        100,
        40,
        content_hash(0x06),
        1_700_000_000_000,
        1_700_000_001_000,
    )
    .expect("the committed record is valid");
    let version = committed_version(record);

    let entry = CatalogEntry::from_committed(&version, MAX_SOURCE_NAME_LEN)
        .expect("the committed version maps to an entry");

    assert_eq!(entry.upload_id, upload_id(0x05));
    assert_eq!(entry.source_name, "  photo.jpg  ");
    assert_eq!(entry.timestamp_ms, 1_700_000_000_000);
    assert_eq!(entry.original_size_bytes, 100);
    assert_eq!(entry.compressed_size_bytes, 40);
    assert_eq!(entry.sha256, content_hash(0x06));
}

#[test]
fn bounds_the_source_name_in_utf8_bytes_without_normalization() {
    let source_name = "  é  ";
    assert_eq!(source_name.len(), 6);

    let entry = CatalogEntry::new(upload_id(0x07), source_name, 0, 0, 0, content_hash(0x08), 6)
        .expect("the source name is exactly at the limit");
    assert_eq!(entry.source_name, source_name);

    let rejected = CatalogEntry::new(upload_id(0x07), source_name, 0, 0, 0, content_hash(0x08), 5);
    assert!(matches!(rejected, Err(CatalogError::SourceNameTooLong)));
}

#[test]
fn rejects_a_capture_time_out_of_i64_range() {
    let record = UploadRecord::new(
        upload_id(0x09),
        "alice",
        "photo.jpg",
        10,
        4,
        content_hash(0x0a),
        u64::MAX,
        1_000,
    )
    .expect("the committed record is valid");
    let version = committed_version(record);

    let result = CatalogEntry::from_committed(&version, MAX_SOURCE_NAME_LEN);

    assert!(matches!(result, Err(CatalogError::TimestampOutOfRange)));
}

#[test]
fn decodes_the_first_page_request() {
    let request = decode_request(&[0x82, 0xf6, 0xf6]).expect("the first-page request decodes");

    assert_eq!(request.snapshot_token, None);
    assert_eq!(request.cursor, None);
}

#[test]
fn decodes_a_continuation_request() {
    let mut encoded = vec![0x82, 0x50];
    encoded.extend(token(0x05));
    encoded.push(0x50);
    encoded.extend(token(0x06));

    let request = decode_request(&encoded).expect("the continuation request decodes");

    assert_eq!(request.snapshot_token, Some(token(0x05)));
    assert_eq!(request.cursor, Some(token(0x06)));
}

#[test]
fn rejects_mixed_request_tokens() {
    let mut null_then_bytes = vec![0x82, 0xf6, 0x50];
    null_then_bytes.extend(token(0x05));
    let mut bytes_then_null = vec![0x82, 0x50];
    bytes_then_null.extend(token(0x05));
    bytes_then_null.push(0xf6);

    for encoded in [null_then_bytes, bytes_then_null] {
        let result = decode_request(&encoded);

        assert!(matches!(result, Err(CatalogError::RequestToken)));
    }
}

#[test]
fn rejects_wrong_request_shapes_and_tokens() {
    let single_element = [0x81, 0xf6];
    assert!(matches!(
        decode_request(&single_element),
        Err(CatalogError::RequestShape)
    ));

    let integer_envelope = [0x00];
    assert!(matches!(
        decode_request(&integer_envelope),
        Err(CatalogError::RequestShape)
    ));

    let mut short_token = vec![0x82, 0x4f];
    short_token.extend([0x05; CATALOG_TOKEN_LEN - 1]);
    short_token.push(0xf6);
    assert!(matches!(
        decode_request(&short_token),
        Err(CatalogError::RequestToken)
    ));

    let text_token = [0x82, 0x61, 0x61, 0xf6];
    assert!(matches!(
        decode_request(&text_token),
        Err(CatalogError::RequestToken)
    ));
}

#[test]
fn rejects_trailing_bytes_after_a_request() {
    let result = decode_request(&[0x82, 0xf6, 0xf6, 0x00]);

    assert!(matches!(result, Err(CatalogError::Decode)));
}
