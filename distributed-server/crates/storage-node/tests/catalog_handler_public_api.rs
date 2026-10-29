//! Integration tests for the public catalog page handler.
//!
//! Covers `FR-CAT-007 / 1`: the handler returns entries in ascending upload id
//! order. Covers `FR-CAT-008 / 1`: every page of one snapshot carries the same
//! snapshot token. Covers `FR-CAT-009 / 1` and `FR-CAT-014 / 2`: cursors are
//! opaque, accepted only with their snapshot token, and rejected after the
//! snapshot expires. Covers `FR-CAT-013 / 2` and `FR-CAT-015 / 1`: a successful
//! response carries `[snapshot_token, next_cursor, entries]` with 16-byte
//! tokens, a cursor on a page with a continuation, and `null` on the last page.
//! Covers `INT-NET-061 / 1`, `INT-NET-064 / 1`, `INT-NET-069 / 1`, and
//! `INT-NET-080 / 1`: the encoded result envelope is `[result_code,
//! result_args_array]`, success uses code `0`, an invalid cursor maps to
//! `0x0004_0001`, and an unknown or expired snapshot maps to `0x0004_0002`,
//! with an empty argument array on every non-zero code.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::Duration;

use ciborium::value::Value;

use storage_node::CatalogHandlerError;
use storage_node::CatalogPageHandler;
use storage_node::CatalogRequest;
use storage_node::CatalogService;
use storage_node::CommittedVersion;
use storage_node::ContentHash;
use storage_node::UploadId;
use storage_node::UploadRecord;
use storage_node::CATALOG_TOKEN_LEN;
use storage_node::UPLOAD_ID_LEN;

/// Source-name byte limit used by the accepted test records.
const MAX_SOURCE_NAME_LEN: usize = 64;

/// Snapshot lifetime used by tests that expect a live snapshot.
const LIVE_SNAPSHOT_TTL_SECONDS: u64 = 30;

/// Snapshot lifetime used by the expiry test.
const EXPIRED_SNAPSHOT_TTL_SECONDS: u64 = 0;

/// Page size used by the handler tests.
const PAGE_SIZE: usize = 2;

/// Client-supplied capture time used by the test records.
const CAPTURED_AT_MS: u64 = 1_700_000_000_000;

/// Server acceptance time used by the test records.
const ACCEPTED_AT_MS: u64 = 1_700_000_100_000;

/// Original uncompressed byte size used by the test records.
const ORIGINAL_SIZE_BYTES: u64 = 100;

/// Compressed payload byte size used by the test records.
const COMPRESSED_SIZE_BYTES: u64 = 40;

/// Number of positions in one catalog entry.
const ENTRY_POSITION_COUNT: usize = 6;

/// Number of arguments in a successful catalog page response.
const SUCCESS_ARGUMENT_COUNT: usize = 3;

/// Successful result code.
const RESULT_CODE_SUCCESS: u32 = 0;

/// Result code for a cursor that does not belong to the snapshot.
const RESULT_CODE_INVALID_CURSOR: u32 = 0x0004_0001;

/// Result code for an unknown or expired snapshot.
const RESULT_CODE_SNAPSHOT_EXPIRED: u32 = 0x0004_0002;

fn version(id: u8) -> CommittedVersion {
    CommittedVersion {
        record: UploadRecord::new(
            UploadId::from_bytes([id; UPLOAD_ID_LEN]),
            "alice",
            &format!("photo-{id}.jpg"),
            ORIGINAL_SIZE_BYTES,
            COMPRESSED_SIZE_BYTES,
            ContentHash::digest(&[id]),
            CAPTURED_AT_MS,
            ACCEPTED_AT_MS,
        )
        .expect("the test record is valid metadata"),
        replicas: [1, 2, 3].into_iter().collect(),
    }
}

fn versions() -> BTreeMap<UploadId, CommittedVersion> {
    [1_u8, 2, 3]
        .into_iter()
        .map(|id| {
            let version = version(id);
            (version.record.upload_id, version)
        })
        .collect()
}

fn service(page_size: usize, ttl_seconds: u64, max_name_len: usize) -> CatalogService {
    CatalogService::new(
        NonZeroUsize::new(page_size).expect("the test page size is non-zero"),
        Duration::from_secs(ttl_seconds),
        NonZeroUsize::new(max_name_len).expect("the test source-name limit is non-zero"),
    )
}

fn decode_response(bytes: &[u8]) -> (u32, Value) {
    let mut slice = bytes;
    let value: Value =
        ciborium::de::from_reader(&mut slice).expect("the handler response is valid CBOR");
    assert!(
        slice.is_empty(),
        "the handler response must not have trailing bytes"
    );

    let Value::Array(elements) = value else {
        panic!("the handler response must be a CBOR array");
    };
    let [result_code, arguments] = elements
        .try_into()
        .expect("the handler response must be a two-element CBOR array");
    let Value::Integer(result_code) = result_code else {
        panic!("the handler response must start with an integer result code");
    };
    let result_code = u32::try_from(result_code).expect("the result code must fit a u32");

    (result_code, arguments)
}

fn expect_array(value: &Value) -> &[Value] {
    let Value::Array(elements) = value else {
        panic!("the value must be a CBOR array");
    };
    elements
}

fn token(value: &Value) -> [u8; CATALOG_TOKEN_LEN] {
    let Value::Bytes(bytes) = value else {
        panic!("a catalog token must be a CBOR byte string");
    };
    bytes
        .as_slice()
        .try_into()
        .expect("a catalog token must be 16 bytes")
}

fn next_cursor(value: &Value) -> Option<[u8; CATALOG_TOKEN_LEN]> {
    match value {
        Value::Null => None,
        value => Some(token(value)),
    }
}

fn success_arguments(value: &Value) -> &[Value] {
    let arguments = expect_array(value);
    assert_eq!(
        arguments.len(),
        SUCCESS_ARGUMENT_COUNT,
        "a successful page must carry three arguments"
    );
    arguments
}

fn request_first_page_tokens(
    handler: &mut CatalogPageHandler,
    versions: &BTreeMap<UploadId, CommittedVersion>,
) -> ([u8; CATALOG_TOKEN_LEN], [u8; CATALOG_TOKEN_LEN]) {
    let response = handler
        .handle(
            versions,
            &CatalogRequest {
                snapshot_token: None,
                cursor: None,
            },
        )
        .expect("the first page request is handled");
    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_SUCCESS);
    let arguments = success_arguments(&arguments);

    (
        token(&arguments[0]),
        next_cursor(&arguments[1]).expect("the first page of a three-entry catalog continues"),
    )
}

fn assert_entry(value: &Value, committed: &CommittedVersion) {
    let entry = expect_array(value);
    assert_eq!(entry.len(), ENTRY_POSITION_COUNT);
    assert_eq!(
        entry[0],
        Value::Bytes(committed.record.upload_id.as_bytes().to_vec())
    );
    assert_eq!(
        entry[1],
        Value::Text(committed.record.original_name.clone())
    );
    assert_eq!(
        entry[2],
        Value::Integer(
            i64::try_from(committed.record.captured_at_ms)
                .expect("the capture time fits milliseconds")
                .into()
        )
    );
    assert_eq!(entry[3], Value::Integer(committed.record.size_bytes.into()));
    assert_eq!(
        entry[4],
        Value::Integer(committed.record.compressed_size_bytes.into())
    );
    assert_eq!(
        entry[5],
        Value::Bytes(committed.record.content_hash.as_bytes().to_vec())
    );
}

#[test]
fn first_page_request_returns_a_success_page() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        LIVE_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));

    let response = handler
        .handle(
            &versions,
            &CatalogRequest {
                snapshot_token: None,
                cursor: None,
            },
        )
        .expect("the first page request is handled");

    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_SUCCESS);
    let arguments = success_arguments(&arguments);

    let snapshot_token = token(&arguments[0]);
    let cursor =
        next_cursor(&arguments[1]).expect("the first page of a three-entry catalog continues");
    assert_ne!(snapshot_token, cursor);

    let entries = expect_array(&arguments[2]);
    assert_eq!(entries.len(), PAGE_SIZE);
    for (entry, id) in entries.iter().zip([1_u8, 2]) {
        let committed = versions
            .get(&UploadId::from_bytes([id; UPLOAD_ID_LEN]))
            .expect("each entry has a committed version");
        assert_entry(entry, committed);
    }
}

#[test]
fn continuation_returns_the_last_page() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        LIVE_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));
    let (snapshot_token, cursor) = request_first_page_tokens(&mut handler, &versions);

    let response = handler
        .handle(
            &versions,
            &CatalogRequest {
                snapshot_token: Some(snapshot_token),
                cursor: Some(cursor),
            },
        )
        .expect("the continuation request is handled");

    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_SUCCESS);
    let arguments = success_arguments(&arguments);
    assert_eq!(token(&arguments[0]), snapshot_token);
    assert_eq!(next_cursor(&arguments[1]), None);

    let entries = expect_array(&arguments[2]);
    assert_eq!(entries.len(), 1);
    let committed = versions
        .get(&UploadId::from_bytes([3; UPLOAD_ID_LEN]))
        .expect("the last page entry has a committed version");
    assert_entry(&entries[0], committed);
}

#[test]
fn unknown_snapshot_maps_to_snapshot_expired() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        LIVE_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));

    let response = handler
        .handle(
            &versions,
            &CatalogRequest {
                snapshot_token: Some([0xaa; CATALOG_TOKEN_LEN]),
                cursor: Some([0xbb; CATALOG_TOKEN_LEN]),
            },
        )
        .expect("an unknown snapshot is reported as a protocol code");

    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_SNAPSHOT_EXPIRED);
    assert!(expect_array(&arguments).is_empty());
}

#[test]
fn unknown_cursor_maps_to_invalid_cursor() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        LIVE_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));
    let (snapshot_token, _) = request_first_page_tokens(&mut handler, &versions);

    let response = handler
        .handle(
            &versions,
            &CatalogRequest {
                snapshot_token: Some(snapshot_token),
                cursor: Some([0xbb; CATALOG_TOKEN_LEN]),
            },
        )
        .expect("an unknown cursor is reported as a protocol code");

    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_INVALID_CURSOR);
    assert!(expect_array(&arguments).is_empty());
}

#[test]
fn expired_snapshot_maps_to_snapshot_expired() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        EXPIRED_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));
    let (snapshot_token, cursor) = request_first_page_tokens(&mut handler, &versions);

    let response = handler
        .handle(
            &versions,
            &CatalogRequest {
                snapshot_token: Some(snapshot_token),
                cursor: Some(cursor),
            },
        )
        .expect("an expired snapshot is reported as a protocol code");

    let (result_code, arguments) = decode_response(&response);
    assert_eq!(result_code, RESULT_CODE_SNAPSHOT_EXPIRED);
    assert!(expect_array(&arguments).is_empty());
}

#[test]
fn mixed_tokens_are_rejected() {
    let versions = versions();
    let mut handler = CatalogPageHandler::new(service(
        PAGE_SIZE,
        LIVE_SNAPSHOT_TTL_SECONDS,
        MAX_SOURCE_NAME_LEN,
    ));

    let token_only = handler.handle(
        &versions,
        &CatalogRequest {
            snapshot_token: Some([0xaa; CATALOG_TOKEN_LEN]),
            cursor: None,
        },
    );
    assert!(matches!(
        token_only,
        Err(CatalogHandlerError::InvalidTokens)
    ));

    let cursor_only = handler.handle(
        &versions,
        &CatalogRequest {
            snapshot_token: None,
            cursor: Some([0xbb; CATALOG_TOKEN_LEN]),
        },
    );
    assert!(matches!(
        cursor_only,
        Err(CatalogHandlerError::InvalidTokens)
    ));
}
