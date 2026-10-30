//! Integration tests for the public inter-server blob-get message codecs.
//!
//! Covers `INT-NET-098 / 1`: the request is `[13, [login, upload_id]]` with the
//! login text and the external upload id as a 16-byte byte string. Covers
//! `INT-NET-099 / 1`: the result is `[14, [result_code, result_args]]`, with
//! `[metadata, transfer_compressed_size]` on success and an empty argument
//! array on every non-zero code. Covers `INT-NET-100 / 1`: a data frame is
//! `[15, [payload_length]]` followed by exactly that many compressed bytes.
//! Covers `INT-NET-116 / 1`: the request keeps the
//! external upload id that the data frames of one get must match, and the
//! success result carries the compressed transfer size. Covers `INT-NET-119 / 1`:
//! a non-zero result code never carries result arguments. Covers
//! `INT-NET-128 / 1`: the success metadata carries `original_size` and the
//! transfer size is bounded by `B(original_size)`.

use std::io::Cursor;
use std::num::NonZeroUsize;

use ciborium::value::Value;

use storage_node::decode_blob_get;
use storage_node::decode_blob_get_data_frame;
use storage_node::decode_blob_get_result;
use storage_node::encode_blob_get;
use storage_node::encode_blob_get_data_frame;
use storage_node::encode_blob_get_result;
use storage_node::BlobGet;
use storage_node::BlobGetData;
use storage_node::BlobGetResult;
use storage_node::BlobMetadata;
use storage_node::ClientLimits;
use storage_node::ContentHash;
use storage_node::UploadId;
use storage_node::CONTENT_HASH_LEN;
use storage_node::UPLOAD_ID_LEN;

/// Login used by the accepted test messages.
const LOGIN: &str = "alice";
/// Maximum accepted source-name length in UTF-8 bytes.
const MAX_SOURCE_NAME_LEN: usize = 64;
/// Maximum accepted original or compressed size in bytes.
const MAX_FILE_BYTES: u64 = 1000;
/// Message type of the blob-get request envelope.
const BLOB_GET_TYPE: u32 = 13;
/// Message type of the blob-get result envelope.
const BLOB_GET_RESULT_TYPE: u32 = 14;
/// Message type of a blob-get data frame.
const BLOB_GET_DATA_TYPE: u32 = 15;
/// Number of positions in a blob-get data argument array.
const DATA_ARGUMENT_COUNT: usize = 1;
/// Byte offset of the payload-length integer inside the canonical data header.
const DATA_LENGTH_OFFSET: usize = 3;
/// Compressed payload used by the accepted data frame.
const COMPRESSED_PAYLOAD: &[u8] = b"compressed";
/// Unassigned message type used by the rejection tests.
const UNKNOWN_MESSAGE_TYPE: u32 = 99;
/// Successful result code.
const RESULT_CODE_SUCCESS: u32 = 0;
/// Non-zero result code used by the failure tests.
const RESULT_CODE_FAILURE: u32 = 0x0008_0001;
/// Number of positions in a blob-get request argument array.
const REQUEST_ARGUMENT_COUNT: usize = 2;
/// Number of positions in a blob metadata array.
const METADATA_FIELD_COUNT: usize = 5;
/// Number of positions in a blob-get result argument array.
const RESULT_ARGUMENT_COUNT: usize = 2;
/// Number of positions in a successful blob-get result argument array.
const SUCCESS_ARGUMENT_COUNT: usize = 2;
/// Compressed transfer size used by the accepted success result.
const TRANSFER_COMPRESSED_SIZE: u64 = 4;
/// One byte above `B(2) = 2 + floor(2 / 256) + 64`.
const TRANSFER_ABOVE_BOUND: u64 = 67;

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

fn upload_id() -> UploadId {
    UploadId::from_bytes([0x05; UPLOAD_ID_LEN])
}

fn metadata() -> BlobMetadata {
    BlobMetadata {
        source_name: "a".to_owned(),
        timestamp_ms: -1,
        original_size_bytes: 2,
        compressed_size_bytes: 3,
        sha256: ContentHash::from_bytes([0x04; CONTENT_HASH_LEN]),
    }
}

fn request() -> BlobGet {
    BlobGet {
        login: LOGIN.to_owned(),
        upload_id: upload_id(),
    }
}

fn success_result() -> BlobGetResult {
    BlobGetResult {
        result_code: RESULT_CODE_SUCCESS,
        metadata: Some(metadata()),
        transfer_compressed_size: Some(TRANSFER_COMPRESSED_SIZE),
    }
}

fn failure_result() -> BlobGetResult {
    BlobGetResult {
        result_code: RESULT_CODE_FAILURE,
        metadata: None,
        transfer_compressed_size: None,
    }
}

fn encode_value(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).expect("the CBOR value encodes");
    bytes
}

fn decode_value(bytes: &[u8]) -> Value {
    let mut slice = bytes;
    let value = ciborium::de::from_reader(&mut slice).expect("the bytes are one CBOR value");
    assert!(
        slice.is_empty(),
        "the message must not have trailing bytes after its logical value"
    );
    value
}

fn decode_prefix(bytes: &[u8]) -> Value {
    let mut cursor = Cursor::new(bytes);
    ciborium::de::from_reader(&mut cursor).expect("the bytes start with one CBOR value")
}

fn expect_array(value: &Value) -> &[Value] {
    let Value::Array(elements) = value else {
        panic!("the value must be a CBOR array");
    };
    elements
}

fn encode_envelope(message_type: u32, arguments: Value) -> Vec<u8> {
    encode_value(&Value::Array(vec![
        Value::Integer(message_type.into()),
        arguments,
    ]))
}

fn data_header(payload_length: u32) -> Vec<u8> {
    encode_envelope(
        BLOB_GET_DATA_TYPE,
        Value::Array(vec![Value::Integer(payload_length.into())]),
    )
}

fn metadata_value(
    source_name: Value,
    timestamp_ms: Value,
    original_size: Value,
    compressed_size: Value,
    sha256: Value,
) -> Value {
    Value::Array(vec![
        source_name,
        timestamp_ms,
        original_size,
        compressed_size,
        sha256,
    ])
}

fn valid_metadata_value() -> Value {
    metadata_value(
        Value::Text("a".to_owned()),
        Value::Integer((-1i64).into()),
        Value::Integer(2u64.into()),
        Value::Integer(3u64.into()),
        Value::Bytes(vec![0x04; CONTENT_HASH_LEN]),
    )
}

fn request_envelope(login: Value, upload_id: Value) -> Vec<u8> {
    encode_envelope(BLOB_GET_TYPE, Value::Array(vec![login, upload_id]))
}

fn result_envelope(result_code: u32, result_args: Value) -> Vec<u8> {
    encode_envelope(
        BLOB_GET_RESULT_TYPE,
        Value::Array(vec![Value::Integer(result_code.into()), result_args]),
    )
}

#[test]
fn round_trips_blob_get() {
    let request = request();

    let encoded = encode_blob_get(&request, &limits()).expect("the request is within the limits");
    let decoded = decode_blob_get(&encoded, &limits()).expect("the encoded request decodes");

    assert_eq!(decoded, request);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope.len(), 2);
    assert_eq!(envelope[0], Value::Integer(BLOB_GET_TYPE.into()));

    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments.len(), REQUEST_ARGUMENT_COUNT);
    assert_eq!(arguments[0], Value::Text(LOGIN.to_owned()));
    assert_eq!(arguments[1], Value::Bytes(upload_id().as_bytes().to_vec()));
}

#[test]
fn rejects_invalid_blob_get() {
    assert!(encode_blob_get(
        &BlobGet {
            login: String::new(),
            upload_id: upload_id(),
        },
        &limits(),
    )
    .is_err());

    let mut invalid_login = request();
    invalid_login.login = "bad login".to_owned();
    assert!(encode_blob_get(&invalid_login, &limits()).is_err());

    let invalid_login_value = request_envelope(
        Value::Text("bad login".to_owned()),
        Value::Bytes(upload_id().as_bytes().to_vec()),
    );
    assert!(decode_blob_get(&invalid_login_value, &limits()).is_err());

    let empty_login_value = request_envelope(
        Value::Text(String::new()),
        Value::Bytes(upload_id().as_bytes().to_vec()),
    );
    assert!(decode_blob_get(&empty_login_value, &limits()).is_err());

    let not_an_array = encode_value(&Value::Text(LOGIN.to_owned()));
    assert!(decode_blob_get(&not_an_array, &limits()).is_err());

    let short_envelope = encode_value(&Value::Array(vec![Value::Integer(BLOB_GET_TYPE.into())]));
    assert!(decode_blob_get(&short_envelope, &limits()).is_err());

    let long_envelope = encode_value(&Value::Array(vec![
        Value::Integer(BLOB_GET_TYPE.into()),
        Value::Array(vec![
            Value::Text(LOGIN.to_owned()),
            Value::Bytes(upload_id().as_bytes().to_vec()),
        ]),
        Value::Integer(0u64.into()),
    ]));
    assert!(decode_blob_get(&long_envelope, &limits()).is_err());

    let short_arguments = encode_envelope(
        BLOB_GET_TYPE,
        Value::Array(vec![Value::Text(LOGIN.to_owned())]),
    );
    assert!(decode_blob_get(&short_arguments, &limits()).is_err());

    let malformed_upload_id = request_envelope(
        Value::Text(LOGIN.to_owned()),
        Value::Bytes(vec![0x05; UPLOAD_ID_LEN - 1]),
    );
    assert!(decode_blob_get(&malformed_upload_id, &limits()).is_err());

    let text_upload_id = request_envelope(
        Value::Text(LOGIN.to_owned()),
        Value::Text("not-bytes".to_owned()),
    );
    assert!(decode_blob_get(&text_upload_id, &limits()).is_err());

    let unknown_type = encode_envelope(
        UNKNOWN_MESSAGE_TYPE,
        Value::Array(vec![
            Value::Text(LOGIN.to_owned()),
            Value::Bytes(upload_id().as_bytes().to_vec()),
        ]),
    );
    assert!(decode_blob_get(&unknown_type, &limits()).is_err());

    let mut noncanonical =
        encode_blob_get(&request(), &limits()).expect("the test request encodes");
    noncanonical.insert(1, 0x18);
    assert!(decode_blob_get(&noncanonical, &limits()).is_err());

    let mut trailing = encode_blob_get(&request(), &limits()).expect("the test request encodes");
    trailing.push(0);
    assert!(decode_blob_get(&trailing, &limits()).is_err());
}

#[test]
fn round_trips_blob_get_result() {
    let success = success_result();
    let encoded = encode_blob_get_result(&success, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the success result is within the limits");
    let decoded = decode_blob_get_result(&encoded, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the encoded success result decodes");

    assert_eq!(decoded, success);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope.len(), 2);
    assert_eq!(envelope[0], Value::Integer(BLOB_GET_RESULT_TYPE.into()));

    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments.len(), RESULT_ARGUMENT_COUNT);
    assert_eq!(arguments[0], Value::Integer(RESULT_CODE_SUCCESS.into()));
    let success_arguments = expect_array(&arguments[1]);
    assert_eq!(success_arguments.len(), SUCCESS_ARGUMENT_COUNT);

    let metadata = expect_array(&success_arguments[0]);
    assert_eq!(metadata.len(), METADATA_FIELD_COUNT);
    assert_eq!(metadata[0], Value::Text("a".to_owned()));
    assert_eq!(metadata[1], Value::Integer((-1i64).into()));
    assert_eq!(metadata[2], Value::Integer(2u64.into()));
    assert_eq!(metadata[3], Value::Integer(3u64.into()));
    assert_eq!(metadata[4], Value::Bytes(vec![0x04; CONTENT_HASH_LEN]));
    assert_eq!(
        success_arguments[1],
        Value::Integer(TRANSFER_COMPRESSED_SIZE.into())
    );

    let failure = failure_result();
    let encoded = encode_blob_get_result(&failure, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the failure result encodes");
    let decoded = decode_blob_get_result(&encoded, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the encoded failure result decodes");

    assert_eq!(decoded, failure);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope[0], Value::Integer(BLOB_GET_RESULT_TYPE.into()));
    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments[0], Value::Integer(RESULT_CODE_FAILURE.into()));
    assert!(expect_array(&arguments[1]).is_empty());
}

#[test]
fn rejects_invalid_blob_get_result() {
    for metadata in [None, Some(metadata())] {
        let result = BlobGetResult {
            result_code: RESULT_CODE_SUCCESS,
            metadata,
            transfer_compressed_size: None,
        };
        assert!(encode_blob_get_result(&result, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());
    }

    let result = BlobGetResult {
        result_code: RESULT_CODE_SUCCESS,
        metadata: None,
        transfer_compressed_size: Some(TRANSFER_COMPRESSED_SIZE),
    };
    assert!(encode_blob_get_result(&result, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    for result in [
        BlobGetResult {
            result_code: RESULT_CODE_FAILURE,
            metadata: Some(metadata()),
            transfer_compressed_size: None,
        },
        BlobGetResult {
            result_code: RESULT_CODE_FAILURE,
            metadata: None,
            transfer_compressed_size: Some(TRANSFER_COMPRESSED_SIZE),
        },
        BlobGetResult {
            result_code: RESULT_CODE_FAILURE,
            metadata: Some(metadata()),
            transfer_compressed_size: Some(TRANSFER_COMPRESSED_SIZE),
        },
    ] {
        assert!(encode_blob_get_result(&result, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());
    }

    for transfer_compressed_size in [0, TRANSFER_ABOVE_BOUND] {
        let result = BlobGetResult {
            result_code: RESULT_CODE_SUCCESS,
            metadata: Some(metadata()),
            transfer_compressed_size: Some(transfer_compressed_size),
        };
        assert!(encode_blob_get_result(&result, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());
    }

    let mut overlong_name = success_result();
    overlong_name
        .metadata
        .as_mut()
        .expect("the success result carries metadata")
        .source_name = "a".repeat(MAX_SOURCE_NAME_LEN + 1);
    assert!(encode_blob_get_result(&overlong_name, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let mut empty_name = success_result();
    empty_name
        .metadata
        .as_mut()
        .expect("the success result carries metadata")
        .source_name
        .clear();
    assert!(encode_blob_get_result(&empty_name, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let mut overlong_original = success_result();
    overlong_original
        .metadata
        .as_mut()
        .expect("the success result carries metadata")
        .original_size_bytes = MAX_FILE_BYTES + 1;
    assert!(
        encode_blob_get_result(&overlong_original, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err()
    );

    let mut overlong_compressed = success_result();
    overlong_compressed
        .metadata
        .as_mut()
        .expect("the success result carries metadata")
        .compressed_size_bytes = MAX_FILE_BYTES + 1;
    assert!(
        encode_blob_get_result(&overlong_compressed, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err()
    );

    assert!(decode_blob_get_result(
        &result_envelope(RESULT_CODE_SUCCESS, Value::Array(Vec::new())),
        MAX_SOURCE_NAME_LEN,
        MAX_FILE_BYTES,
    )
    .is_err());

    assert!(decode_blob_get_result(
        &result_envelope(
            RESULT_CODE_SUCCESS,
            Value::Array(vec![valid_metadata_value()]),
        ),
        MAX_SOURCE_NAME_LEN,
        MAX_FILE_BYTES,
    )
    .is_err());

    assert!(decode_blob_get_result(
        &result_envelope(
            RESULT_CODE_SUCCESS,
            Value::Array(vec![
                valid_metadata_value(),
                Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
                Value::Integer(0u64.into()),
            ]),
        ),
        MAX_SOURCE_NAME_LEN,
        MAX_FILE_BYTES,
    )
    .is_err());

    for result_args in [
        Value::Array(vec![valid_metadata_value()]),
        Value::Array(vec![
            valid_metadata_value(),
            Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
        ]),
    ] {
        assert!(decode_blob_get_result(
            &result_envelope(RESULT_CODE_FAILURE, result_args),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES,
        )
        .is_err());
    }

    for transfer_compressed_size in [0, TRANSFER_ABOVE_BOUND] {
        assert!(decode_blob_get_result(
            &result_envelope(
                RESULT_CODE_SUCCESS,
                Value::Array(vec![
                    valid_metadata_value(),
                    Value::Integer(transfer_compressed_size.into()),
                ]),
            ),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES,
        )
        .is_err());
    }

    for invalid_metadata in [
        metadata_value(
            Value::Text("a".repeat(MAX_SOURCE_NAME_LEN + 1)),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x04; CONTENT_HASH_LEN]),
        ),
        metadata_value(
            Value::Text(String::new()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x04; CONTENT_HASH_LEN]),
        ),
        metadata_value(
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer((MAX_FILE_BYTES + 1).into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x04; CONTENT_HASH_LEN]),
        ),
        metadata_value(
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer((MAX_FILE_BYTES + 1).into()),
            Value::Bytes(vec![0x04; CONTENT_HASH_LEN]),
        ),
        Value::Array(vec![
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
        ]),
    ] {
        assert!(decode_blob_get_result(
            &result_envelope(
                RESULT_CODE_SUCCESS,
                Value::Array(vec![
                    invalid_metadata,
                    Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
                ]),
            ),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES,
        )
        .is_err());
    }

    let non_array_arguments = result_envelope(RESULT_CODE_SUCCESS, Value::Text("args".to_owned()));
    assert!(
        decode_blob_get_result(&non_array_arguments, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err()
    );

    let short_envelope = encode_value(&Value::Array(vec![Value::Integer(
        BLOB_GET_RESULT_TYPE.into(),
    )]));
    assert!(decode_blob_get_result(&short_envelope, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let long_envelope = encode_value(&Value::Array(vec![
        Value::Integer(BLOB_GET_RESULT_TYPE.into()),
        Value::Array(vec![
            Value::Integer(RESULT_CODE_SUCCESS.into()),
            Value::Array(vec![
                valid_metadata_value(),
                Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
            ]),
        ]),
        Value::Integer(0u64.into()),
    ]));
    assert!(decode_blob_get_result(&long_envelope, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let short_result_arguments = encode_envelope(
        BLOB_GET_RESULT_TYPE,
        Value::Array(vec![Value::Integer(RESULT_CODE_SUCCESS.into())]),
    );
    assert!(
        decode_blob_get_result(&short_result_arguments, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .is_err()
    );

    let long_result_arguments = encode_envelope(
        BLOB_GET_RESULT_TYPE,
        Value::Array(vec![
            Value::Integer(RESULT_CODE_SUCCESS.into()),
            Value::Array(vec![
                valid_metadata_value(),
                Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
            ]),
            Value::Integer(0u64.into()),
        ]),
    );
    assert!(
        decode_blob_get_result(&long_result_arguments, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .is_err()
    );

    let unknown_type = encode_envelope(
        UNKNOWN_MESSAGE_TYPE,
        Value::Array(vec![
            Value::Integer(RESULT_CODE_SUCCESS.into()),
            Value::Array(vec![
                valid_metadata_value(),
                Value::Integer(TRANSFER_COMPRESSED_SIZE.into()),
            ]),
        ]),
    );
    assert!(decode_blob_get_result(&unknown_type, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let mut noncanonical =
        encode_blob_get_result(&success_result(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .expect("the test success result encodes");
    noncanonical.insert(1, 0x18);
    assert!(decode_blob_get_result(&noncanonical, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());

    let mut trailing =
        encode_blob_get_result(&success_result(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .expect("the test success result encodes");
    trailing.push(0);
    assert!(decode_blob_get_result(&trailing, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err());
}

#[test]
fn round_trips_blob_get_data_frame() {
    let encoded = encode_blob_get_data_frame(COMPRESSED_PAYLOAD)
        .expect("a non-empty data payload is accepted");
    let (header, payload) =
        decode_blob_get_data_frame(&encoded).expect("the encoded data frame decodes");

    let expected_length =
        u32::try_from(COMPRESSED_PAYLOAD.len()).expect("the test payload fits a u32");
    assert_eq!(
        header,
        BlobGetData {
            payload_length: expected_length,
        }
    );
    assert_eq!(payload, COMPRESSED_PAYLOAD);

    let envelope = decode_prefix(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope.len(), 2);
    assert_eq!(envelope[0], Value::Integer(BLOB_GET_DATA_TYPE.into()));

    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments.len(), DATA_ARGUMENT_COUNT);
    assert_eq!(arguments[0], Value::Integer(expected_length.into()));
}

#[test]
fn rejects_invalid_blob_get_data_frame() {
    assert!(encode_blob_get_data_frame(&[]).is_err());

    let zero_header = data_header(0);
    assert!(decode_blob_get_data_frame(&zero_header).is_err());

    let encoded =
        encode_blob_get_data_frame(COMPRESSED_PAYLOAD).expect("the test data frame encodes");

    let truncated = &encoded[..encoded.len() - 1];
    assert!(decode_blob_get_data_frame(truncated).is_err());

    let mut extra = encoded.clone();
    extra.push(0);
    assert!(decode_blob_get_data_frame(&extra).is_err());

    let header_only =
        data_header(u32::try_from(COMPRESSED_PAYLOAD.len()).expect("the test payload fits a u32"));
    assert!(decode_blob_get_data_frame(&header_only).is_err());

    let mut noncanonical = encoded;
    noncanonical.insert(DATA_LENGTH_OFFSET, 0x18);
    assert!(decode_blob_get_data_frame(&noncanonical).is_err());
}

#[test]
fn debug_redacts_login() {
    let rendered = format!("{:?}", request());
    assert!(rendered.contains("login_len"));
    assert!(!rendered.contains(LOGIN));
}
