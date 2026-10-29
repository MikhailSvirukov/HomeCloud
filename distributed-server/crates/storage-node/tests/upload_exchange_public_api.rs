//! Integration tests exercising only the public upload request exchange codecs.
//!
//! Covers `INT-NET-051 / 3`: the client logical header is `[4, login, password,
//! [source_name, timestamp_ms, original_size_bytes, compressed_size_bytes,
//! sha256]]`, and the argument array is determined by the operation code and
//! validated strictly. Covers `INT-NET-052 / 4`: the upload arguments follow
//! that order, the timestamp is a signed 64-bit CBOR integer, the original and
//! compressed sizes are unsigned CBOR integers no larger than the configured
//! maximum file size, and the SHA-256 is a 32-byte CBOR byte string. Covers
//! `INT-NET-053 / 2`: the source name is a UTF-8 text string preserved exactly
//! without trimming or Unicode normalization, bounded by the configured
//! source-name byte limit. Covers `INT-NET-055 / 2`: operation `4` is a
//! `uint32` in the deterministic minimal representation, and unknown operation
//! codes are rejected.

use ciborium::value::Value;

use storage_node::decode_upload_request;
use storage_node::encode_upload_request;
use storage_node::ContentHash;
use storage_node::UploadExchangeError;
use storage_node::UploadRequest;
use storage_node::CONTENT_HASH_LEN;

const LOGIN: &str = "alice";
const PASSWORD: &str = " s ";
const MAX_SOURCE_NAME_LEN: usize = 64;
const MAX_FILE_BYTES: u64 = 100;
const UPLOAD_OPERATION: u32 = 4;

fn upload_request(
    source_name: &str,
    timestamp_ms: i64,
    original_size_bytes: u64,
    compressed_size_bytes: u64,
    hash_byte: u8,
) -> UploadRequest {
    UploadRequest {
        source_name: source_name.to_owned(),
        timestamp_ms,
        original_size_bytes,
        compressed_size_bytes,
        sha256: ContentHash::from_bytes([hash_byte; CONTENT_HASH_LEN]),
    }
}

fn encode_cbor(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).expect("the CBOR value encodes");
    bytes
}

fn upload_arguments(
    source_name: Value,
    timestamp: Value,
    original_size: Value,
    compressed_size: Value,
    sha256: Value,
) -> Value {
    Value::Array(vec![
        source_name,
        timestamp,
        original_size,
        compressed_size,
        sha256,
    ])
}

fn valid_arguments() -> Value {
    upload_arguments(
        Value::Text("a".to_owned()),
        Value::Integer((-1i64).into()),
        Value::Integer(2u64.into()),
        Value::Integer(3u64.into()),
        Value::Bytes(vec![0x02; CONTENT_HASH_LEN]),
    )
}

fn upload_header(operation: Value, login: Value, password: Value, arguments: Value) -> Vec<u8> {
    encode_cbor(&Value::Array(vec![operation, login, password, arguments]))
}

fn valid_upload_header() -> Vec<u8> {
    upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        valid_arguments(),
    )
}

#[test]
fn encodes_the_upload_request_as_a_golden_array() {
    let request = upload_request("a", -1, 2, 3, 0x02);

    let encoded = encode_upload_request(
        LOGIN,
        PASSWORD,
        &request,
        MAX_SOURCE_NAME_LEN,
        MAX_FILE_BYTES,
    )
    .expect("the request is within the source-name and file-size limits");

    let mut expected = vec![0x84, 0x04, 0x65];
    expected.extend_from_slice(LOGIN.as_bytes());
    expected.push(0x63);
    expected.extend_from_slice(PASSWORD.as_bytes());
    expected.extend_from_slice(&[0x85, 0x61, b'a']);
    expected.extend_from_slice(&[0x20, 0x02, 0x03, 0x58, 0x20]);
    expected.extend([0x02; CONTENT_HASH_LEN]);
    assert_eq!(encoded, expected);

    let decoded = decode_upload_request(&encoded, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
        .expect("the encoded upload request decodes");

    assert_eq!(decoded.login(), LOGIN);
    assert_eq!(decoded.password(), PASSWORD);
    let decoded_request = decoded.request();
    assert_eq!(decoded_request.source_name, "a");
    assert_eq!(decoded_request.timestamp_ms, -1);
    assert_eq!(decoded_request.original_size_bytes, 2);
    assert_eq!(decoded_request.compressed_size_bytes, 3);
    assert_eq!(
        decoded_request.sha256,
        ContentHash::from_bytes([0x02; CONTENT_HASH_LEN])
    );
}

#[test]
fn preserves_a_multibyte_source_name_without_normalization() {
    let source_name = "  é  ";
    assert_eq!(source_name.len(), 6);
    let request = upload_request(source_name, 0, 1, 1, 0x01);

    let encoded = encode_upload_request(LOGIN, PASSWORD, &request, 6, MAX_FILE_BYTES)
        .expect("the six-byte source name is exactly at the limit");
    let decoded = decode_upload_request(&encoded, 6, MAX_FILE_BYTES)
        .expect("the six-byte source name is exactly at the limit");

    assert_eq!(decoded.login(), LOGIN);
    assert_eq!(decoded.password(), PASSWORD);
    assert_eq!(decoded.request().source_name, source_name);
}

#[test]
fn bounds_the_source_name_and_sizes() {
    let too_long = upload_request("abcdef", 1, 1, 1, 0x01);
    assert!(matches!(
        encode_upload_request(LOGIN, PASSWORD, &too_long, 5, MAX_FILE_BYTES),
        Err(UploadExchangeError::InvalidSourceName)
    ));

    let empty_name = upload_request("", 1, 1, 1, 0x01);
    assert!(matches!(
        encode_upload_request(
            LOGIN,
            PASSWORD,
            &empty_name,
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        ),
        Err(UploadExchangeError::InvalidSourceName)
    ));

    let original_too_large = upload_request("a", 1, 11, 1, 0x01);
    assert!(matches!(
        encode_upload_request(
            LOGIN,
            PASSWORD,
            &original_too_large,
            MAX_SOURCE_NAME_LEN,
            10
        ),
        Err(UploadExchangeError::InvalidSizes)
    ));

    let compressed_too_large = upload_request("a", 1, 1, 11, 0x01);
    assert!(matches!(
        encode_upload_request(
            LOGIN,
            PASSWORD,
            &compressed_too_large,
            MAX_SOURCE_NAME_LEN,
            10
        ),
        Err(UploadExchangeError::InvalidSizes)
    ));

    let six_byte_name = upload_request("abcdef", 1, 1, 1, 0x01);
    let encoded = encode_upload_request(LOGIN, PASSWORD, &six_byte_name, 6, MAX_FILE_BYTES)
        .expect("the six-byte source name is exactly at the limit");
    assert!(matches!(
        decode_upload_request(&encoded, 5, MAX_FILE_BYTES),
        Err(UploadExchangeError::InvalidSourceName)
    ));

    let large_sizes = upload_request("a", 1, 11, 11, 0x01);
    let encoded = encode_upload_request(LOGIN, PASSWORD, &large_sizes, MAX_SOURCE_NAME_LEN, 11)
        .expect("the sizes are at the configured limit");
    assert!(matches!(
        decode_upload_request(&encoded, MAX_SOURCE_NAME_LEN, 10),
        Err(UploadExchangeError::InvalidSizes)
    ));
}

#[test]
fn rejects_invalid_upload_headers() {
    let unknown_operation = upload_header(
        Value::Integer(3u32.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        valid_arguments(),
    );
    assert!(
        decode_upload_request(&unknown_operation, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "operation 3 must not decode as an upload request"
    );

    let login_with_space = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text("al ice".to_owned()),
        Value::Text(PASSWORD.to_owned()),
        valid_arguments(),
    );
    assert!(
        decode_upload_request(&login_with_space, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a login containing a space must be rejected"
    );

    let password_as_bytes = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Bytes(PASSWORD.as_bytes().to_vec()),
        valid_arguments(),
    );
    assert!(
        decode_upload_request(&password_as_bytes, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a password that is not a CBOR text string must be rejected"
    );

    let short_top_level = encode_cbor(&Value::Array(vec![
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
    ]));
    assert!(
        decode_upload_request(&short_top_level, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a three-position logical header must be rejected"
    );

    let long_top_level = encode_cbor(&Value::Array(vec![
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        valid_arguments(),
        Value::Null,
    ]));
    assert!(
        decode_upload_request(&long_top_level, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a five-position logical header must be rejected"
    );

    let short_arguments = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        Value::Array(vec![
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
        ]),
    );
    assert!(
        decode_upload_request(&short_arguments, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "an upload argument array with four positions must be rejected"
    );

    let long_arguments = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        Value::Array(vec![
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x02; CONTENT_HASH_LEN]),
            Value::Null,
        ]),
    );
    assert!(
        decode_upload_request(&long_arguments, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "an upload argument array with six positions must be rejected"
    );

    let non_integer_timestamp = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        upload_arguments(
            Value::Text("a".to_owned()),
            Value::Text("now".to_owned()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x02; CONTENT_HASH_LEN]),
        ),
    );
    assert!(
        decode_upload_request(&non_integer_timestamp, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a text timestamp must be rejected"
    );

    let negative_original_size = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        upload_arguments(
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer((-1i64).into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x02; CONTENT_HASH_LEN]),
        ),
    );
    assert!(
        decode_upload_request(&negative_original_size, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .is_err(),
        "a negative original size must be rejected"
    );

    let negative_compressed_size = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        upload_arguments(
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer((-1i64).into()),
            Value::Bytes(vec![0x02; CONTENT_HASH_LEN]),
        ),
    );
    assert!(
        decode_upload_request(
            &negative_compressed_size,
            MAX_SOURCE_NAME_LEN,
            MAX_FILE_BYTES
        )
        .is_err(),
        "a negative compressed size must be rejected"
    );

    let short_hash = upload_header(
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        upload_arguments(
            Value::Text("a".to_owned()),
            Value::Integer((-1i64).into()),
            Value::Integer(2u64.into()),
            Value::Integer(3u64.into()),
            Value::Bytes(vec![0x02; CONTENT_HASH_LEN - 1]),
        ),
    );
    assert!(
        decode_upload_request(&short_hash, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "a 31-byte hash must be rejected"
    );

    let mut trailing_bytes = valid_upload_header();
    trailing_bytes.push(0xf6);
    assert!(
        decode_upload_request(&trailing_bytes, MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES).is_err(),
        "bytes after the CBOR value must be rejected"
    );
}
