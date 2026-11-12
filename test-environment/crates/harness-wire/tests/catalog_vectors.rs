//! Golden vectors for the v1 catalog page answer and page request.
//!
//! Every test pins exact bytes from [`contracts/wire-v1.md`] §§5–7.1 so
//! component implementations can consume these vectors as external
//! conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::catalog::{CatalogEntry, CatalogError, CatalogPage, CatalogPageRequest};
use harness_wire::cbor::{self, Value};
use harness_wire::result::ResultEnvelope;

/// `CFG-CL-032` source-name limit used by the contract vectors.
const SOURCE_NAME_LIMIT: usize = 4096;

const UPLOAD_ID: [u8; 16] = [0x44; 16];
const SOURCE_NAME: &str = "photo.jpg";
const TIMESTAMP_MS: i64 = 1_700_000_000_000;
const ORIGINAL_SIZE_BYTES: u64 = 1024;
const COMPRESSED_SIZE_BYTES: u64 = 512;
const SHA256: [u8; 32] = [0x55; 32];
const SNAPSHOT_TOKEN: [u8; 16] = [0xAA; 16];
const CURSOR: [u8; 16] = [0xBB; 16];

/// Offset of `original_size_bytes` in a row with the contract's fixed prefix.
const SIZES_OFFSET: usize = 37;

/// The contract entry.
fn entry() -> CatalogEntry {
    CatalogEntry {
        upload_id: UPLOAD_ID,
        source_name: SOURCE_NAME.to_owned(),
        timestamp_ms: TIMESTAMP_MS,
        original_size_bytes: ORIGINAL_SIZE_BYTES,
        compressed_size_bytes: COMPRESSED_SIZE_BYTES,
        sha256: SHA256,
    }
}

/// The contract entry as a CBOR value.
fn entry_row() -> Value {
    entry().to_value(SOURCE_NAME_LIMIT).expect("entry is valid")
}

/// Real bytes of the contract entry row.
fn entry_row_bytes() -> Vec<u8> {
    let mut bytes = vec![0x86, 0x50];
    bytes.extend_from_slice(&UPLOAD_ID);
    bytes.push(0x69);
    bytes.extend_from_slice(b"photo.jpg");
    bytes.extend_from_slice(&[0x1B, 0x00, 0x00, 0x01, 0x8B, 0xCF, 0xE5, 0x68, 0x00]);
    bytes.extend_from_slice(&[0x19, 0x04, 0x00, 0x19, 0x02, 0x00, 0x58, 0x20]);
    bytes.extend_from_slice(&SHA256);
    bytes
}

/// The entry row with `name` as its source name.
fn entry_row_with_source_name(name: &str) -> Value {
    with_position(entry_row(), 1, Value::Text(name.to_owned()))
}

/// Replaces one position of a CBOR array with `replacement`.
fn with_position(value: Value, position: usize, replacement: Value) -> Value {
    let Value::Array(mut items) = value else {
        panic!("expected a CBOR array");
    };
    items[position] = replacement;
    Value::Array(items)
}

/// A middle page of the contract snapshot.
fn middle_page() -> CatalogPage {
    CatalogPage {
        snapshot_token: SNAPSHOT_TOKEN,
        next_cursor: Some(CURSOR),
        entries: vec![entry()],
    }
}

/// The last page of the contract snapshot.
fn last_page() -> CatalogPage {
    CatalogPage {
        snapshot_token: SNAPSHOT_TOKEN,
        next_cursor: None,
        entries: vec![entry()],
    }
}

/// Literal bytes of a success page answer with `cursor` as its next cursor.
fn expected_page_bytes(cursor: Option<[u8; 16]>) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x00, 0x83, 0x50];
    bytes.extend_from_slice(&SNAPSHOT_TOKEN);
    match cursor {
        Some(cursor) => {
            bytes.push(0x50);
            bytes.extend_from_slice(&cursor);
        }
        None => bytes.push(0xF6),
    }
    bytes.push(0x81);
    bytes.extend_from_slice(&entry_row_bytes());
    bytes
}

/// Encodes a page answer as the `code = 0` result envelope.
fn page_answer(page: &CatalogPage) -> Vec<u8> {
    ResultEnvelope {
        code: 0,
        args: page
            .to_args(SOURCE_NAME_LIMIT)
            .expect("page arguments are valid"),
    }
    .encode()
    .expect("envelope is valid")
}

/// Decodes a page answer from signed-contract bytes.
fn decode_page(bytes: &[u8]) -> CatalogPage {
    let envelope = ResultEnvelope::decode(&cbor::decode(bytes).expect("answer is valid CBOR"))
        .expect("envelope is valid");
    CatalogPage::from_args(&envelope.args, SOURCE_NAME_LIMIT).expect("page is valid")
}

/// `FR-CAT-012/3`, `TST-025/2`.
#[test]
fn catalog_entry_matches_contract_vector() {
    let row = entry().to_value(SOURCE_NAME_LIMIT).expect("entry is valid");

    assert_eq!(cbor::encode(&row), entry_row_bytes());
    assert_eq!(entry_row_bytes().len(), 77);

    let decoded = cbor::decode(&entry_row_bytes()).expect("row is valid CBOR");
    assert_eq!(
        CatalogEntry::from_value(&decoded, SOURCE_NAME_LIMIT),
        Ok(entry())
    );
}

/// `FR-CAT-013/2`, `TST-026/2`: a middle page carries a 16-byte cursor.
#[test]
fn middle_catalog_page_matches_contract_vector() {
    let page = middle_page();
    let expected = expected_page_bytes(Some(CURSOR));

    assert_eq!(expected.len(), 115);
    assert_eq!(page_answer(&page), expected);
    assert_eq!(decode_page(&expected), page);
}

/// `FR-CAT-013/2`, `TST-026/2`: the last page carries a `null` cursor.
#[test]
fn last_catalog_page_has_null_cursor() {
    let page = last_page();
    let expected = expected_page_bytes(None);

    assert_eq!(expected.len(), 99);
    assert_eq!(page_answer(&page), expected);

    let decoded = decode_page(&expected);
    assert_eq!(decoded, page);
    assert_eq!(decoded.next_cursor, None);
    assert_eq!(decoded.entries, vec![entry()]);
}

/// `FR-CAT-013/2`, `TST-026/2`: every page of one snapshot repeats its token
/// unchanged and returns the cursor unchanged.
#[test]
fn pages_of_one_snapshot_keep_the_token() {
    let middle_bytes = page_answer(&middle_page());
    let last_bytes = page_answer(&last_page());

    assert_eq!(&middle_bytes[4..20], &SNAPSHOT_TOKEN);
    assert_eq!(&last_bytes[4..20], &SNAPSHOT_TOKEN);

    let middle = decode_page(&middle_bytes);
    let last = decode_page(&last_bytes);
    assert_eq!(middle.snapshot_token, SNAPSHOT_TOKEN);
    assert_eq!(last.snapshot_token, SNAPSHOT_TOKEN);
    assert_eq!(middle.next_cursor, Some(CURSOR));
}

/// `TST-028/1`, `INT-NET-080/1`: the first page requests `[null, null]`.
#[test]
fn first_catalog_request_matches_contract_vector() {
    let request = CatalogPageRequest {
        snapshot_token: None,
        cursor: None,
    };
    let args = request.to_args().expect("request is valid");

    assert_eq!(args, vec![Value::Null, Value::Null]);
    assert_eq!(cbor::encode(&Value::Array(args)), vec![0x82, 0xF6, 0xF6]);
    assert_eq!(
        CatalogPageRequest::from_args(&[Value::Null, Value::Null]),
        Ok(request)
    );
}

/// `TST-028/1`: a continuation page repeats the token and cursor unchanged.
#[test]
fn continuation_catalog_request_matches_contract_vector() {
    let request = CatalogPageRequest {
        snapshot_token: Some(SNAPSHOT_TOKEN),
        cursor: Some(CURSOR),
    };
    let expected = {
        let mut bytes = vec![0x82, 0x50];
        bytes.extend_from_slice(&SNAPSHOT_TOKEN);
        bytes.push(0x50);
        bytes.extend_from_slice(&CURSOR);
        bytes
    };
    let args = request.to_args().expect("request is valid");

    assert_eq!(cbor::encode(&Value::Array(args)), expected);
    let Value::Array(decoded) = cbor::decode(&expected).expect("request is valid CBOR") else {
        panic!("request arguments are an array");
    };
    assert_eq!(CatalogPageRequest::from_args(&decoded), Ok(request));
}

/// `TST-028/1`, `INT-NET-080/1`: the token and cursor are both present or both
/// `null`.
#[test]
fn catalog_request_rejects_a_partial_cursor_pair() {
    assert_eq!(
        CatalogPageRequest::from_args(&[Value::Bytes(SNAPSHOT_TOKEN.to_vec()), Value::Null]),
        Err(CatalogError::IncompleteCursorPair)
    );
    assert_eq!(
        CatalogPageRequest::from_args(&[Value::Null, Value::Bytes(CURSOR.to_vec())]),
        Err(CatalogError::IncompleteCursorPair)
    );
    assert_eq!(
        CatalogPageRequest {
            snapshot_token: Some(SNAPSHOT_TOKEN),
            cursor: None,
        }
        .to_args(),
        Err(CatalogError::IncompleteCursorPair)
    );
    assert_eq!(
        CatalogPageRequest {
            snapshot_token: None,
            cursor: Some(CURSOR),
        }
        .to_args(),
        Err(CatalogError::IncompleteCursorPair)
    );
}

/// `CFG-CL-032`, `TST-034/1`: the source-name limit counts UTF-8 bytes and is
/// inclusive.
#[test]
fn source_name_accepts_exactly_the_configured_limit() {
    let accepted = CatalogEntry {
        source_name: "a".repeat(SOURCE_NAME_LIMIT),
        ..entry()
    };
    let row = accepted
        .to_value(SOURCE_NAME_LIMIT)
        .expect("a 4096-byte name is allowed");
    assert_eq!(
        CatalogEntry::from_value(&row, SOURCE_NAME_LIMIT),
        Ok(accepted)
    );

    let too_long = CatalogEntry {
        source_name: "a".repeat(SOURCE_NAME_LIMIT + 1),
        ..entry()
    };
    assert_eq!(
        too_long.to_value(SOURCE_NAME_LIMIT),
        Err(CatalogError::SourceNameTooLong {
            actual: 4097,
            limit: 4096,
        })
    );
    assert_eq!(
        CatalogEntry::from_value(
            &entry_row_with_source_name(&"a".repeat(SOURCE_NAME_LIMIT + 1)),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::SourceNameTooLong {
            actual: 4097,
            limit: 4096,
        })
    );

    let two_byte_name = "é".repeat(SOURCE_NAME_LIMIT / 2);
    let two_byte_row = CatalogEntry {
        source_name: two_byte_name.clone(),
        ..entry()
    }
    .to_value(SOURCE_NAME_LIMIT)
    .expect("a 4096-byte UTF-8 name is allowed");
    assert_eq!(
        CatalogEntry::from_value(&two_byte_row, SOURCE_NAME_LIMIT)
            .expect("a 4096-byte UTF-8 name decodes")
            .source_name,
        two_byte_name
    );

    assert_eq!(
        CatalogEntry::from_value(
            &entry_row_with_source_name(&"é".repeat(SOURCE_NAME_LIMIT / 2 + 1)),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::SourceNameTooLong {
            actual: 4098,
            limit: 4096,
        })
    );
}

/// `TST-034/1`: the source name is not Unicode-normalized.
#[test]
fn source_name_round_trips_unicode_without_normalization() {
    let cyrillic = CatalogEntry {
        source_name: "фото.jpg".to_owned(),
        ..entry()
    };
    let cyrillic_row = cyrillic.to_value(SOURCE_NAME_LIMIT).expect("row is valid");
    assert_eq!(
        CatalogEntry::from_value(&cyrillic_row, SOURCE_NAME_LIMIT),
        Ok(cyrillic)
    );

    let composed = CatalogEntry {
        source_name: "é".to_owned(),
        ..entry()
    };
    let decomposed = CatalogEntry {
        source_name: "e\u{0301}".to_owned(),
        ..entry()
    };
    let composed_bytes = cbor::encode(&composed.to_value(SOURCE_NAME_LIMIT).expect("row is valid"));
    let decomposed_bytes = cbor::encode(
        &decomposed
            .to_value(SOURCE_NAME_LIMIT)
            .expect("row is valid"),
    );

    assert_ne!(composed_bytes, decomposed_bytes);
    assert_eq!(
        CatalogEntry::from_value(
            &cbor::decode(&composed_bytes).expect("row is valid CBOR"),
            SOURCE_NAME_LIMIT,
        ),
        Ok(composed)
    );
    assert_eq!(
        CatalogEntry::from_value(
            &cbor::decode(&decomposed_bytes).expect("row is valid CBOR"),
            SOURCE_NAME_LIMIT,
        ),
        Ok(decomposed)
    );
}

/// `FR-CAT-013/2`, `TST-026/2`: tokens and cursors are exactly 16-byte byte
/// strings.
#[test]
fn catalog_token_and_cursor_lengths_are_enforced() {
    for length in [15, 17] {
        assert_eq!(
            CatalogPage::from_args(
                &[
                    Value::Bytes(vec![0xAA; length]),
                    Value::Null,
                    Value::Array(vec![]),
                ],
                SOURCE_NAME_LIMIT,
            ),
            Err(CatalogError::InvalidLength("snapshot_token"))
        );
    }
    assert_eq!(
        CatalogPage::from_args(
            &[
                Value::Bytes(SNAPSHOT_TOKEN.to_vec()),
                Value::Bytes(vec![0xBB; 15]),
                Value::Array(vec![]),
            ],
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::InvalidLength("next_cursor"))
    );
    assert_eq!(
        CatalogPageRequest::from_args(&[
            Value::Bytes(SNAPSHOT_TOKEN.to_vec()),
            Value::Bytes(vec![0xBB; 15]),
        ]),
        Err(CatalogError::InvalidLength("cursor"))
    );
}

/// `FR-CAT-012/3`, `TST-025/2`: the timestamp is a signed 64-bit integer.
#[test]
fn catalog_timestamp_boundaries() {
    for timestamp_ms in [i64::MIN, i64::MAX] {
        let bounded = CatalogEntry {
            timestamp_ms,
            ..entry()
        };
        let row = bounded.to_value(SOURCE_NAME_LIMIT).expect("row is valid");
        assert_eq!(
            CatalogEntry::from_value(
                &cbor::decode(&cbor::encode(&row)).expect("row is valid CBOR"),
                SOURCE_NAME_LIMIT,
            ),
            Ok(bounded)
        );
    }

    let maximum = CatalogEntry {
        timestamp_ms: i64::MAX,
        ..entry()
    };
    let maximum_bytes = cbor::encode(&maximum.to_value(SOURCE_NAME_LIMIT).expect("row is valid"));
    assert_eq!(
        &maximum_bytes[28..37],
        &[0x1B, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    );

    assert_eq!(
        CatalogEntry::from_value(
            &with_position(entry_row(), 2, Value::Uint(u64::MAX)),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::TimestampOutOfRange(u64::MAX))
    );
}

/// `TST-025/2`: both size fields accept the full `uint64` range.
#[test]
fn catalog_sizes_accept_u64_max() {
    let maximum = CatalogEntry {
        original_size_bytes: u64::MAX,
        compressed_size_bytes: u64::MAX,
        ..entry()
    };
    let row = maximum.to_value(SOURCE_NAME_LIMIT).expect("row is valid");
    let encoded = cbor::encode(&row);

    let maximum_bytes = [0x1B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    assert_eq!(&encoded[SIZES_OFFSET..SIZES_OFFSET + 9], &maximum_bytes);
    assert_eq!(
        &encoded[SIZES_OFFSET + 9..SIZES_OFFSET + 18],
        &maximum_bytes
    );
    assert_eq!(
        CatalogEntry::from_value(
            &cbor::decode(&encoded).expect("row is valid CBOR"),
            SOURCE_NAME_LIMIT,
        ),
        Ok(maximum)
    );
}

/// `TST-025/2`, `TST-026/2`: entries, page answers, and requests have fixed
/// array lengths.
#[test]
fn catalog_rejects_wrong_shapes() {
    let Value::Array(items) = entry_row() else {
        panic!("entry row is an array");
    };
    assert_eq!(
        CatalogEntry::from_value(&Value::Array(items[..5].to_vec()), SOURCE_NAME_LIMIT),
        Err(CatalogError::WrongEntryShape)
    );
    let mut longer = items.clone();
    longer.push(Value::Null);
    assert_eq!(
        CatalogEntry::from_value(&Value::Array(longer), SOURCE_NAME_LIMIT),
        Err(CatalogError::WrongEntryShape)
    );

    assert_eq!(
        CatalogPage::from_args(
            &[Value::Bytes(SNAPSHOT_TOKEN.to_vec()), Value::Null],
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::WrongPageShape)
    );
    assert_eq!(
        CatalogPage::from_args(
            &[
                Value::Bytes(SNAPSHOT_TOKEN.to_vec()),
                Value::Null,
                Value::Array(vec![]),
                Value::Null,
            ],
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::WrongPageShape)
    );

    assert_eq!(
        CatalogPageRequest::from_args(&[Value::Null]),
        Err(CatalogError::WrongRequestShape)
    );
    assert_eq!(
        CatalogPageRequest::from_args(&[Value::Null, Value::Null, Value::Null]),
        Err(CatalogError::WrongRequestShape)
    );
}

/// `TST-025/2`: each entry position keeps its published CBOR type.
#[test]
fn catalog_rejects_wrong_types() {
    assert_eq!(
        CatalogEntry::from_value(
            &with_position(entry_row(), 1, Value::Uint(1)),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::WrongType("source_name"))
    );
    assert_eq!(
        CatalogEntry::from_value(
            &with_position(entry_row(), 2, Value::Text("0".to_owned())),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::WrongType("timestamp_ms"))
    );
    assert_eq!(
        CatalogEntry::from_value(
            &with_position(entry_row(), 3, Value::Text("1024".to_owned())),
            SOURCE_NAME_LIMIT,
        ),
        Err(CatalogError::WrongType("original_size_bytes"))
    );
    assert_eq!(
        CatalogEntry::from_value(&Value::Uint(0), SOURCE_NAME_LIMIT),
        Err(CatalogError::WrongEntryShape)
    );
}
