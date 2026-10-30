//! Integration tests for the public inter-server blob-put message codecs.
//!
//! Covers `INT-NET-095 / 1`: the begin message is `[10, [login, upload_id,
//! metadata, transfer_compressed_size]]` with metadata
//! `[source_name, timestamp_ms, original_size, compressed_size, sha256]`.
//! Covers `INT-NET-096 / 1`: the data message is `[11, [payload_length]]`
//! followed by exactly `payload_length` compressed bytes and an empty payload
//! is rejected. Covers `INT-NET-097 / 1`: the result message is
//! `[12, [result_code, result_args]]`, with `[upload_id]` on success and an
//! empty argument array on every non-zero code. Covers `INT-NET-114 / 1`: the
//! logical messages carry the external upload id unchanged. Covers
//! `INT-NET-119 / 1`: a non-zero result code never carries result arguments.
//! Covers `INT-NET-128 / 1`: the begin metadata carries `original_size` and the
//! transfer size is bounded by `B(original_size)`.

use std::io::Cursor;
use std::num::NonZeroUsize;

use ciborium::value::Value;

use storage_node::decode_blob_put_begin;
use storage_node::decode_blob_put_data_frame;
use storage_node::decode_blob_put_result;
use storage_node::encode_blob_put_begin;
use storage_node::encode_blob_put_data_frame;
use storage_node::encode_blob_put_result;
use storage_node::BlobMetadata;
use storage_node::BlobPutBegin;
use storage_node::BlobPutData;
use storage_node::BlobPutError;
use storage_node::BlobPutResult;
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
/// Message type of the blob-put begin envelope.
const BLOB_PUT_BEGIN_TYPE: u32 = 10;
/// Message type of the blob-put data envelope.
const BLOB_PUT_DATA_TYPE: u32 = 11;
/// Message type of the blob-put result envelope.
const BLOB_PUT_RESULT_TYPE: u32 = 12;
/// Unassigned message type used by the rejection test.
const UNKNOWN_MESSAGE_TYPE: u32 = 99;
/// Successful result code.
const RESULT_CODE_SUCCESS: u32 = 0;
/// Non-zero result code used by the failure test.
const RESULT_CODE_FAILURE: u32 = 0x0008_0001;
/// Number of positions in a blob-put begin argument array.
const BEGIN_ARGUMENT_COUNT: usize = 4;
/// Number of positions in a blob metadata array.
const METADATA_FIELD_COUNT: usize = 5;
/// Compressed payload used by the data-frame round trip.
const COMPRESSED_PAYLOAD: &[u8] = b"compressed";

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

fn begin() -> BlobPutBegin {
    BlobPutBegin {
        login: LOGIN.to_owned(),
        upload_id: upload_id(),
        metadata: metadata(),
        transfer_compressed_size: 4,
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

fn begin_envelope(
    login: Value,
    upload_id: Value,
    metadata: Value,
    transfer_compressed_size: Value,
) -> Vec<u8> {
    encode_envelope(
        BLOB_PUT_BEGIN_TYPE,
        Value::Array(vec![login, upload_id, metadata, transfer_compressed_size]),
    )
}

fn result_envelope(result_code: u32, result_args: Value) -> Vec<u8> {
    encode_envelope(
        BLOB_PUT_RESULT_TYPE,
        Value::Array(vec![Value::Integer(result_code.into()), result_args]),
    )
}

fn data_header(payload_length: u32) -> Vec<u8> {
    encode_envelope(
        BLOB_PUT_DATA_TYPE,
        Value::Array(vec![Value::Integer(payload_length.into())]),
    )
}

fn expect_begin_error(bytes: &[u8], expected: fn(&BlobPutError) -> bool) {
    let error = decode_blob_put_begin(bytes, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect_err("the malformed begin message is rejected");
    assert!(
        expected(&error),
        "the begin message fails with the expected error"
    );
}

#[test]
fn round_trips_blob_put_begin() {
    let begin = begin();

    let encoded = encode_blob_put_begin(&begin, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the begin message is within the configured limits");
    let decoded = decode_blob_put_begin(&encoded, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the encoded begin message decodes");

    assert_eq!(decoded, begin);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope.len(), 2);
    assert_eq!(envelope[0], Value::Integer(BLOB_PUT_BEGIN_TYPE.into()));

    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments.len(), BEGIN_ARGUMENT_COUNT);
    assert_eq!(arguments[0], Value::Text(LOGIN.to_owned()));
    assert_eq!(arguments[1], Value::Bytes(upload_id().as_bytes().to_vec()));

    let metadata = expect_array(&arguments[2]);
    assert_eq!(metadata.len(), METADATA_FIELD_COUNT);
    assert_eq!(metadata[0], Value::Text("a".to_owned()));
    assert_eq!(metadata[1], Value::Integer((-1i64).into()));
    assert_eq!(metadata[2], Value::Integer(2u64.into()));
    assert_eq!(metadata[3], Value::Integer(3u64.into()));
    assert_eq!(metadata[4], Value::Bytes(vec![0x04; CONTENT_HASH_LEN]));
    assert_eq!(arguments[3], Value::Integer(4u64.into()));
}

#[test]
fn enforces_begin_name_login_and_size_limits() {
    let mut overlong_name = begin();
    overlong_name.metadata.source_name = "a".repeat(MAX_SOURCE_NAME_LEN + 1);
    assert!(matches!(
        encode_blob_put_begin(
            &overlong_name,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::InvalidSourceName)
    ));

    let mut empty_name = begin();
    empty_name.metadata.source_name.clear();
    assert!(matches!(
        encode_blob_put_begin(&empty_name, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
        Err(BlobPutError::InvalidSourceName)
    ));

    let mut empty_login = begin();
    empty_login.login.clear();
    assert!(matches!(
        encode_blob_put_begin(&empty_login, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
        Err(BlobPutError::InvalidLogin)
    ));

    let mut invalid_login = begin();
    invalid_login.login = "bad login".to_owned();
    assert!(matches!(
        encode_blob_put_begin(
            &invalid_login,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::InvalidLogin)
    ));

    let mut original_too_large = begin();
    original_too_large.metadata.original_size_bytes = MAX_FILE_BYTES + 1;
    assert!(matches!(
        encode_blob_put_begin(
            &original_too_large,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::InvalidSizes)
    ));

    let mut compressed_too_large = begin();
    compressed_too_large.metadata.compressed_size_bytes = MAX_FILE_BYTES + 1;
    assert!(matches!(
        encode_blob_put_begin(
            &compressed_too_large,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::InvalidSizes)
    ));

    let mut zero_transfer = begin();
    zero_transfer.transfer_compressed_size = 0;
    assert!(matches!(
        encode_blob_put_begin(
            &zero_transfer,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::InvalidTransferSize)
    ));

    // B(2) = 2 + floor(2 / 256) + 64 = 66.
    let mut at_bound = begin();
    at_bound.transfer_compressed_size = 66;
    let encoded = encode_blob_put_begin(&at_bound, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the transfer size is exactly at B(original_size)");
    let decoded = decode_blob_put_begin(&encoded, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the transfer size is exactly at B(original_size)");
    assert_eq!(decoded.transfer_compressed_size, 66);

    let mut above_bound = begin();
    above_bound.transfer_compressed_size = 67;
    assert!(matches!(
        encode_blob_put_begin(&above_bound, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
        Err(BlobPutError::InvalidTransferSize)
    ));

    let malformed_hash = metadata_value(
        Value::Text("a".to_owned()),
        Value::Integer((-1i64).into()),
        Value::Integer(2u64.into()),
        Value::Integer(3u64.into()),
        Value::Bytes(vec![0x04; CONTENT_HASH_LEN - 1]),
    );
    expect_begin_error(
        &begin_envelope(
            Value::Text(LOGIN.to_owned()),
            Value::Bytes(upload_id().as_bytes().to_vec()),
            malformed_hash,
            Value::Integer(4u64.into()),
        ),
        |error| matches!(error, BlobPutError::InvalidHash),
    );

    expect_begin_error(
        &begin_envelope(
            Value::Text(LOGIN.to_owned()),
            Value::Bytes(vec![0x05; UPLOAD_ID_LEN - 1]),
            valid_metadata_value(),
            Value::Integer(4u64.into()),
        ),
        |error| matches!(error, BlobPutError::InvalidUploadId),
    );

    expect_begin_error(
        &begin_envelope(
            Value::Text(LOGIN.to_owned()),
            Value::Bytes(upload_id().as_bytes().to_vec()),
            valid_metadata_value(),
            Value::Integer(0u64.into()),
        ),
        |error| matches!(error, BlobPutError::InvalidTransferSize),
    );
}

#[test]
fn round_trips_blob_put_data_frame() {
    let encoded = encode_blob_put_data_frame(COMPRESSED_PAYLOAD)
        .expect("a non-empty data payload is accepted");
    let (header, payload) =
        decode_blob_put_data_frame(&encoded).expect("the encoded data frame decodes");

    assert_eq!(
        header,
        BlobPutData {
            payload_length: u32::try_from(COMPRESSED_PAYLOAD.len())
                .expect("the test payload fits a u32"),
        }
    );
    assert_eq!(payload, COMPRESSED_PAYLOAD);

    assert!(matches!(
        encode_blob_put_data_frame(&[]),
        Err(BlobPutError::InvalidPayloadLength)
    ));

    let zero_header = data_header(0);
    assert!(matches!(
        decode_blob_put_data_frame(&zero_header),
        Err(BlobPutError::InvalidPayloadLength)
    ));

    let truncated = &encoded[..encoded.len() - 1];
    assert!(matches!(
        decode_blob_put_data_frame(truncated),
        Err(BlobPutError::PayloadMismatch)
    ));

    let mut extra = encoded.clone();
    extra.push(0);
    assert!(matches!(
        decode_blob_put_data_frame(&extra),
        Err(BlobPutError::PayloadMismatch)
    ));

    let header_only =
        data_header(u32::try_from(COMPRESSED_PAYLOAD.len()).expect("the test payload fits a u32"));
    assert!(matches!(
        decode_blob_put_data_frame(&header_only),
        Err(BlobPutError::PayloadMismatch)
    ));

    let envelope = decode_prefix(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope[0], Value::Integer(BLOB_PUT_DATA_TYPE.into()));
    let arguments = expect_array(&envelope[1]);
    assert_eq!(
        arguments,
        [Value::Integer(
            u32::try_from(COMPRESSED_PAYLOAD.len())
                .expect("the test payload fits a u32")
                .into()
        )]
    );
}

#[test]
fn round_trips_blob_put_result() {
    let success = BlobPutResult {
        result_code: RESULT_CODE_SUCCESS,
        upload_id: Some(upload_id()),
    };
    let encoded = encode_blob_put_result(&success).expect("a success result carries the upload id");
    let decoded = decode_blob_put_result(&encoded).expect("the success result decodes");
    assert_eq!(decoded, success);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope[0], Value::Integer(BLOB_PUT_RESULT_TYPE.into()));
    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments[0], Value::Integer(RESULT_CODE_SUCCESS.into()));
    assert_eq!(
        expect_array(&arguments[1]),
        [Value::Bytes(upload_id().as_bytes().to_vec())]
    );

    let failure = BlobPutResult {
        result_code: RESULT_CODE_FAILURE,
        upload_id: None,
    };
    let encoded = encode_blob_put_result(&failure).expect("a non-zero result carries no upload id");
    let decoded = decode_blob_put_result(&encoded).expect("the failure result decodes");
    assert_eq!(decoded, failure);

    let envelope = decode_value(&encoded);
    let envelope = expect_array(&envelope);
    assert_eq!(envelope[0], Value::Integer(BLOB_PUT_RESULT_TYPE.into()));
    let arguments = expect_array(&envelope[1]);
    assert_eq!(arguments[0], Value::Integer(RESULT_CODE_FAILURE.into()));
    assert!(expect_array(&arguments[1]).is_empty());

    assert!(matches!(
        encode_blob_put_result(&BlobPutResult {
            result_code: RESULT_CODE_SUCCESS,
            upload_id: None,
        }),
        Err(BlobPutError::InvalidResult)
    ));
    assert!(matches!(
        encode_blob_put_result(&BlobPutResult {
            result_code: RESULT_CODE_FAILURE,
            upload_id: Some(upload_id()),
        }),
        Err(BlobPutError::InvalidResult)
    ));

    assert!(matches!(
        decode_blob_put_result(&result_envelope(
            RESULT_CODE_SUCCESS,
            Value::Array(Vec::new())
        )),
        Err(BlobPutError::InvalidResult)
    ));
    assert!(matches!(
        decode_blob_put_result(&result_envelope(
            RESULT_CODE_FAILURE,
            Value::Array(vec![Value::Bytes(upload_id().as_bytes().to_vec())]),
        )),
        Err(BlobPutError::InvalidResult)
    ));

    let mut trailing = encode_blob_put_result(&success).expect("the success result encodes");
    trailing.push(0);
    assert!(matches!(
        decode_blob_put_result(&trailing),
        Err(BlobPutError::Decode)
    ));
}

#[test]
fn message_type_codes_are_the_assigned_blob_put_codes() {
    let encoded = encode_blob_put_begin(&begin(), &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the begin message encodes");
    let envelope = decode_value(&encoded);
    assert_eq!(
        expect_array(&envelope)[0],
        Value::Integer(BLOB_PUT_BEGIN_TYPE.into())
    );

    let encoded = encode_blob_put_data_frame(COMPRESSED_PAYLOAD).expect("the data frame encodes");
    let envelope = decode_prefix(&encoded);
    assert_eq!(
        expect_array(&envelope)[0],
        Value::Integer(BLOB_PUT_DATA_TYPE.into())
    );

    let encoded = encode_blob_put_result(&BlobPutResult {
        result_code: RESULT_CODE_SUCCESS,
        upload_id: Some(upload_id()),
    })
    .expect("the result message encodes");
    let envelope = decode_value(&encoded);
    assert_eq!(
        expect_array(&envelope)[0],
        Value::Integer(BLOB_PUT_RESULT_TYPE.into())
    );

    let malformed = encode_envelope(UNKNOWN_MESSAGE_TYPE, Value::Array(Vec::new()));
    assert!(matches!(
        decode_blob_put_begin(&malformed, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
        Err(BlobPutError::WrongMessageType)
    ));
}

#[test]
fn rejects_noncanonical_cbor() {
    let encoded_begin =
        encode_blob_put_begin(&begin(), &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .expect("the test begin encodes");
    let mut nonminimal_begin = encoded_begin;
    nonminimal_begin.insert(1, 0x18);
    assert!(matches!(
        decode_blob_put_begin(
            &nonminimal_begin,
            &limits(),
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(BlobPutError::Decode)
    ));

    let encoded_data =
        encode_blob_put_data_frame(COMPRESSED_PAYLOAD).expect("the test data frame encodes");
    let mut nonminimal_data = encoded_data;
    nonminimal_data.insert(3, 0x18);
    assert!(matches!(
        decode_blob_put_data_frame(&nonminimal_data),
        Err(BlobPutError::Decode)
    ));

    let encoded_result = encode_blob_put_result(&BlobPutResult {
        result_code: RESULT_CODE_SUCCESS,
        upload_id: Some(upload_id()),
    })
    .expect("the test result encodes");
    let mut nonminimal_result = encoded_result;
    nonminimal_result.insert(3, 0x18);
    assert!(matches!(
        decode_blob_put_result(&nonminimal_result),
        Err(BlobPutError::Decode)
    ));
}

#[test]
fn debug_redacts_login_and_source_name() {
    let rendered = format!("{:?}", begin());
    assert!(rendered.contains("login_len"));
    assert!(rendered.contains("source_name_len"));
    assert!(!rendered.contains(LOGIN));
    assert!(!rendered.contains("source_name: \"a\""));
}
