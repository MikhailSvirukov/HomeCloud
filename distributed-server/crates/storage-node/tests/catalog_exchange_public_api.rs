//! Integration tests exercising only the public catalog page exchange codecs.
//!
//! Covers `INT-NET-050 / 4`: the external header is a seven-position CBOR array
//! in the fixed order `protocol_version`, `sender_id`, `request_id`,
//! `frame_number`, `upload_id`, `responder_label`, and
//! `bootstrap_signing_public_key`, with the documented null rules and byte-string
//! lengths. Covers `INT-NET-051 / 3` and `INT-NET-080 / 1`: a catalog page
//! request uses the client logical header `[3, login, password,
//! [snapshot_token, cursor]]` directly against a storage node rather than the
//! inter-server `[message_type, args]` envelope. Covers `INT-NET-061 / 1` and
//! `INT-NET-069 / 1`: the response is `[result_code, result_args_array]`, with a
//! non-zero code carrying an empty argument array. Covers `FR-CAT-013 / 2` and
//! `FR-CAT-015 / 1`: a successful page carries `[snapshot_token, next_cursor,
//! entries]`, and the request carries `[snapshot_token, cursor]` with both
//! positions null on the first page. Covers `CFG-CL-029 / 1` and
//! `CFG-CL-031 / 1`: the codecs enforce the configured login and password byte
//! ranges and the responder-label byte limit.

use std::num::NonZeroUsize;

use ciborium::value::Value;

use storage_node::decode_catalog_page_request;
use storage_node::decode_external_header;
use storage_node::encode_catalog_page_request;
use storage_node::encode_catalog_page_response;
use storage_node::encode_external_header;
use storage_node::CatalogEntry;
use storage_node::CatalogExchangeError;
use storage_node::CatalogPage;
use storage_node::CatalogRequest;
use storage_node::ClientLimits;
use storage_node::ContentHash;
use storage_node::ExternalHeader;
use storage_node::UploadId;
use storage_node::BOOTSTRAP_KEY_LEN;
use storage_node::CATALOG_TOKEN_LEN;
use storage_node::CONTENT_HASH_LEN;
use storage_node::REQUEST_ID_LEN;
use storage_node::SENDER_ID_LEN;
use storage_node::UPLOAD_ID_LEN;

const MAX_SOURCE_NAME_LEN: usize = 64;

fn limits() -> ClientLimits {
    let non_zero = |value| NonZeroUsize::new(value).expect("test client limit is non-zero");
    ClientLimits::new(
        non_zero(1),
        non_zero(64),
        non_zero(1),
        non_zero(128),
        non_zero(64),
    )
    .expect("test client limits are consistent")
}

fn encode_cbor(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).expect("the CBOR value encodes");
    bytes
}

fn expect_array(value: &Value) -> &[Value] {
    let Value::Array(elements) = value else {
        panic!("the encoded value must be a CBOR array");
    };
    elements
}

fn header_elements(
    protocol_version: u32,
    sender_id: Option<[u8; SENDER_ID_LEN]>,
    request_id: &[u8],
    frame_number: u32,
    upload_id: Option<[u8; SENDER_ID_LEN]>,
    responder_label: Value,
    bootstrap_signing_public_key: Option<[u8; BOOTSTRAP_KEY_LEN]>,
) -> Vec<Value> {
    vec![
        Value::Integer(protocol_version.into()),
        sender_id.map_or(Value::Null, |id| Value::Bytes(id.to_vec())),
        Value::Bytes(request_id.to_vec()),
        Value::Integer(frame_number.into()),
        upload_id.map_or(Value::Null, |id| Value::Bytes(id.to_vec())),
        responder_label,
        bootstrap_signing_public_key.map_or(Value::Null, |key| Value::Bytes(key.to_vec())),
    ]
}

fn ordinary_header_elements() -> Vec<Value> {
    header_elements(
        1,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Null,
        None,
    )
}

fn single_entry_page() -> CatalogPage {
    let entry = CatalogEntry::new(
        UploadId::from_bytes([0x66; UPLOAD_ID_LEN]),
        "a",
        -1,
        2,
        3,
        ContentHash::from_bytes([0x77; CONTENT_HASH_LEN]),
        MAX_SOURCE_NAME_LEN,
    )
    .expect("the source name is within the limit");

    CatalogPage {
        snapshot_token: [0x55; CATALOG_TOKEN_LEN],
        next_cursor: None,
        entries: vec![entry],
    }
}

#[test]
fn encodes_and_decodes_an_ordinary_external_header() {
    let header = ExternalHeader {
        protocol_version: 1,
        sender_id: Some([0x11; SENDER_ID_LEN]),
        request_id: [0x22; REQUEST_ID_LEN],
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    };

    let encoded = encode_external_header(&header, &limits()).expect("the external header encodes");

    let mut expected = vec![0x87, 0x01, 0x50];
    expected.extend([0x11; SENDER_ID_LEN]);
    expected.push(0x50);
    expected.extend([0x22; REQUEST_ID_LEN]);
    expected.extend([0x00, 0xf6, 0xf6, 0xf6]);
    assert_eq!(encoded, expected);

    let decoded = decode_external_header(&encoded, &limits()).expect("the external header decodes");
    assert_eq!(decoded.protocol_version, 1);
    assert_eq!(decoded.sender_id, Some([0x11; SENDER_ID_LEN]));
    assert_eq!(decoded.request_id, [0x22; REQUEST_ID_LEN]);
    assert_eq!(decoded.frame_number, 0);
    assert_eq!(decoded.upload_id, None);
    assert_eq!(decoded.responder_label, None);
    assert_eq!(decoded.bootstrap_signing_public_key, None);

    let response_header = ExternalHeader {
        protocol_version: 1,
        sender_id: Some([0x11; SENDER_ID_LEN]),
        request_id: [0x22; REQUEST_ID_LEN],
        frame_number: 1,
        upload_id: None,
        responder_label: Some("node-1".to_owned()),
        bootstrap_signing_public_key: None,
    };

    let encoded =
        encode_external_header(&response_header, &limits()).expect("the response header encodes");
    let mut expected = vec![0x87, 0x01, 0x50];
    expected.extend([0x11; SENDER_ID_LEN]);
    expected.push(0x50);
    expected.extend([0x22; REQUEST_ID_LEN]);
    expected.extend([0x01, 0xf6, 0x66]);
    expected.extend_from_slice(b"node-1");
    expected.push(0xf6);
    assert_eq!(encoded, expected);

    let decoded = decode_external_header(&encoded, &limits()).expect("the response header decodes");
    assert_eq!(decoded.protocol_version, 1);
    assert_eq!(decoded.sender_id, Some([0x11; SENDER_ID_LEN]));
    assert_eq!(decoded.request_id, [0x22; REQUEST_ID_LEN]);
    assert_eq!(decoded.frame_number, 1);
    assert_eq!(decoded.upload_id, None);
    assert_eq!(decoded.responder_label.as_deref(), Some("node-1"));
    assert_eq!(decoded.bootstrap_signing_public_key, None);
}

#[test]
fn rejects_malformed_external_headers() {
    let unsupported_version = Value::Array(header_elements(
        2,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Null,
        None,
    ));
    assert!(
        decode_external_header(&encode_cbor(&unsupported_version), &limits()).is_err(),
        "protocol version 2 must be rejected"
    );

    let mut short_array = ordinary_header_elements();
    short_array.pop();
    assert!(
        decode_external_header(&encode_cbor(&Value::Array(short_array)), &limits()).is_err(),
        "a six-position external header must be rejected"
    );

    let missing_sender = Value::Array(header_elements(
        1,
        None,
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Null,
        None,
    ));
    assert!(
        decode_external_header(&encode_cbor(&missing_sender), &limits()).is_err(),
        "a missing sender id without a bootstrap key must be rejected"
    );

    let sender_with_bootstrap_key = Value::Array(header_elements(
        1,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Null,
        Some([0x33; BOOTSTRAP_KEY_LEN]),
    ));
    assert!(
        decode_external_header(&encode_cbor(&sender_with_bootstrap_key), &limits()).is_err(),
        "an assigned sender id with a bootstrap key must be rejected"
    );

    let wrong_request_id_length = Value::Array(header_elements(
        1,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN - 1],
        0,
        None,
        Value::Null,
        None,
    ));
    assert!(
        decode_external_header(&encode_cbor(&wrong_request_id_length), &limits()).is_err(),
        "a request id that is not 16 bytes must be rejected"
    );

    let label_as_bytes = Value::Array(header_elements(
        1,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Bytes(b"node-1".to_vec()),
        None,
    ));
    assert!(
        decode_external_header(&encode_cbor(&label_as_bytes), &limits()).is_err(),
        "a responder label encoded as a byte string must be rejected"
    );

    let mut trailing_bytes = encode_cbor(&Value::Array(ordinary_header_elements()));
    trailing_bytes.push(0x00);
    assert!(
        decode_external_header(&trailing_bytes, &limits()).is_err(),
        "trailing bytes after an external header must be rejected"
    );
}

#[test]
fn encodes_a_first_page_catalog_request_golden() {
    let request = CatalogRequest {
        snapshot_token: None,
        cursor: None,
    };

    let encoded = encode_catalog_page_request("alice", " s ", &request, &limits())
        .expect("the first-page catalog request encodes");

    let mut expected = vec![0x84, 0x03, 0x65];
    expected.extend_from_slice(b"alice");
    expected.push(0x63);
    expected.extend_from_slice(b" s ");
    expected.extend([0x82, 0xf6, 0xf6]);
    assert!(
        encoded == expected,
        "the first-page catalog request does not match the golden bytes"
    );

    let decoded =
        decode_catalog_page_request(&encoded, &limits()).expect("the catalog request decodes");
    assert_eq!(decoded.login(), "alice");
    assert!(
        decoded.password() == " s ",
        "the decoded password preserves the entered text"
    );

    let decoded_request = decoded.request();
    assert_eq!(decoded_request.snapshot_token, None);
    assert_eq!(decoded_request.cursor, None);
}

#[test]
fn decodes_a_continuation_catalog_request() {
    let request = CatalogRequest {
        snapshot_token: Some([0x33; CATALOG_TOKEN_LEN]),
        cursor: Some([0x44; CATALOG_TOKEN_LEN]),
    };

    let encoded = encode_catalog_page_request("bob", "pw", &request, &limits())
        .expect("the continuation catalog request encodes");
    let decoded =
        decode_catalog_page_request(&encoded, &limits()).expect("the catalog request decodes");

    assert_eq!(decoded.login(), "bob");
    assert!(
        decoded.password() == "pw",
        "the decoded password preserves the entered text"
    );

    let decoded_request = decoded.request();
    assert_eq!(
        decoded_request.snapshot_token,
        Some([0x33; CATALOG_TOKEN_LEN])
    );
    assert_eq!(decoded_request.cursor, Some([0x44; CATALOG_TOKEN_LEN]));
}

#[test]
fn rejects_invalid_catalog_requests() {
    let unsupported_operation = Value::Array(vec![
        Value::Integer(2_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(" s ".to_owned()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&unsupported_operation), &limits()).is_err(),
        "operation 2 must not be accepted as a catalog page request"
    );

    let empty_login = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text(String::new()),
        Value::Text(" s ".to_owned()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&empty_login), &limits()).is_err(),
        "an empty login must be rejected"
    );

    let login_with_space = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("a b".to_owned()),
        Value::Text(" s ".to_owned()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&login_with_space), &limits()).is_err(),
        "a login outside the ASCII account set must be rejected"
    );

    let password_as_bytes = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Bytes(b" s ".to_vec()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&password_as_bytes), &limits()).is_err(),
        "a password encoded as a byte string must be rejected"
    );

    let wrong_header_length = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(" s ".to_owned()),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&wrong_header_length), &limits()).is_err(),
        "a three-position client logical header must be rejected"
    );

    let not_an_argument_array = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(" s ".to_owned()),
        Value::Null,
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&not_an_argument_array), &limits()).is_err(),
        "the catalog error for a non-array argument shape must propagate"
    );

    let wrong_token_length = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(" s ".to_owned()),
        Value::Array(vec![
            Value::Bytes(vec![0x33; CATALOG_TOKEN_LEN - 1]),
            Value::Null,
        ]),
    ]);
    assert!(
        decode_catalog_page_request(&encode_cbor(&wrong_token_length), &limits()).is_err(),
        "the catalog error for a short snapshot token must propagate"
    );

    let valid = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(" s ".to_owned()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    let mut trailing_bytes = encode_cbor(&valid);
    trailing_bytes.push(0x00);
    assert!(
        decode_catalog_page_request(&trailing_bytes, &limits()).is_err(),
        "trailing bytes after a catalog request must be rejected"
    );
}

#[test]
fn rejects_logins_outside_the_configured_byte_range() {
    let request = CatalogRequest {
        snapshot_token: None,
        cursor: None,
    };

    assert!(matches!(
        encode_catalog_page_request("", "pw", &request, &limits()),
        Err(CatalogExchangeError::InvalidLogin)
    ));

    let long_login = "a".repeat(limits().login_max_len.get() + 1);
    assert!(matches!(
        encode_catalog_page_request(&long_login, "pw", &request, &limits()),
        Err(CatalogExchangeError::InvalidLogin)
    ));

    let decoded_long_login = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text(long_login),
        Value::Text("pw".to_owned()),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(matches!(
        decode_catalog_page_request(&encode_cbor(&decoded_long_login), &limits()),
        Err(CatalogExchangeError::InvalidLogin)
    ));
}

#[test]
fn rejects_passwords_over_the_configured_byte_limit() {
    let request = CatalogRequest {
        snapshot_token: None,
        cursor: None,
    };

    let long_password = "p".repeat(limits().password_max_len.get() + 1);
    assert!(matches!(
        encode_catalog_page_request("alice", &long_password, &request, &limits()),
        Err(CatalogExchangeError::InvalidPassword)
    ));

    let decoded_long_password = Value::Array(vec![
        Value::Integer(3_u32.into()),
        Value::Text("alice".to_owned()),
        Value::Text(long_password),
        Value::Array(vec![Value::Null, Value::Null]),
    ]);
    assert!(matches!(
        decode_catalog_page_request(&encode_cbor(&decoded_long_password), &limits()),
        Err(CatalogExchangeError::InvalidPassword)
    ));
}

#[test]
fn bounds_responder_labels_to_the_configured_byte_limit() {
    let label_at_limit = "a".repeat(limits().responder_label_max_len.get());
    let header = ExternalHeader {
        protocol_version: 1,
        sender_id: Some([0x11; SENDER_ID_LEN]),
        request_id: [0x22; REQUEST_ID_LEN],
        frame_number: 0,
        upload_id: None,
        responder_label: Some(label_at_limit.clone()),
        bootstrap_signing_public_key: None,
    };

    let encoded = encode_external_header(&header, &limits())
        .expect("a responder label at the configured limit encodes");
    let decoded = decode_external_header(&encoded, &limits())
        .expect("a responder label at the configured limit decodes");
    assert_eq!(
        decoded.responder_label.as_deref(),
        Some(label_at_limit.as_str())
    );

    let over_limit = ExternalHeader {
        responder_label: Some("a".repeat(limits().responder_label_max_len.get() + 1)),
        ..header
    };
    assert!(matches!(
        encode_external_header(&over_limit, &limits()),
        Err(CatalogExchangeError::InvalidResponderLabel)
    ));

    let encoded_over_limit = encode_cbor(&Value::Array(header_elements(
        1,
        Some([0x11; SENDER_ID_LEN]),
        &[0x22; REQUEST_ID_LEN],
        0,
        None,
        Value::Text("a".repeat(limits().responder_label_max_len.get() + 1)),
        None,
    )));
    assert!(matches!(
        decode_external_header(&encoded_over_limit, &limits()),
        Err(CatalogExchangeError::InvalidResponderLabel)
    ));
}

#[test]
fn encodes_a_successful_catalog_page_response() {
    let page = single_entry_page();

    let encoded = encode_catalog_page_response(0, Some(&page))
        .expect("the successful catalog page response encodes");
    let value: Value =
        ciborium::de::from_reader(encoded.as_slice()).expect("the response is valid CBOR");

    let envelope = expect_array(&value);
    assert_eq!(envelope.len(), 2);
    assert_eq!(envelope[0], Value::Integer(0_u32.into()));

    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments.len(), 3);
    assert_eq!(arguments[0], Value::Bytes(vec![0x55; CATALOG_TOKEN_LEN]));
    assert_eq!(arguments[1], Value::Null);

    let entries = expect_array(&arguments[2]);
    assert_eq!(entries.len(), 1);
    let entry = expect_array(&entries[0]);
    assert_eq!(entry.len(), 6);
    assert_eq!(entry[0], Value::Bytes(vec![0x66; UPLOAD_ID_LEN]));
    assert_eq!(entry[1], Value::Text("a".to_owned()));
    assert_eq!(entry[2], Value::Integer((-1_i64).into()));
    assert_eq!(entry[3], Value::Integer(2_u64.into()));
    assert_eq!(entry[4], Value::Integer(3_u64.into()));
    assert_eq!(entry[5], Value::Bytes(vec![0x77; CONTENT_HASH_LEN]));
}

#[test]
fn encodes_an_empty_failure_argument_array() {
    let encoded = encode_catalog_page_response(0x0004_0002, None)
        .expect("the failed catalog page response encodes");
    assert_eq!(encoded, vec![0x82, 0x1a, 0x00, 0x04, 0x00, 0x02, 0x80]);

    let value: Value =
        ciborium::de::from_reader(encoded.as_slice()).expect("the response is valid CBOR");

    assert_eq!(
        value,
        Value::Array(vec![
            Value::Integer(0x0004_0002_u32.into()),
            Value::Array(Vec::new()),
        ])
    );

    assert!(matches!(
        encode_catalog_page_response(0, None),
        Err(CatalogExchangeError::Encode)
    ));

    let page = single_entry_page();
    assert!(matches!(
        encode_catalog_page_response(0x0004_0002, Some(&page)),
        Err(CatalogExchangeError::Encode)
    ));
}
