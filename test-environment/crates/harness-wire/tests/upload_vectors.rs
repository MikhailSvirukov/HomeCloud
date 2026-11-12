//! Golden vectors for the v1 upload metadata and upload result structures.
//!
//! Every test pins exact bytes from [`contracts/wire-v1.md`] and the v1
//! requirement table so component implementations can consume these vectors as
//! external conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::cbor::{self, Value};
use harness_wire::result::ResultEnvelope;
use harness_wire::upload::{UploadError, UploadMetadata, UploadResult};

/// `CFG-CL-032` source-name limit used by the contract vectors.
const SOURCE_NAME_LIMIT: usize = 4096;
/// Unbounded upload-size limit used by the contract vectors.
const SIZE_LIMIT: u64 = u64::MAX;

const SOURCE_NAME: &str = "photo.jpg";
const TIMESTAMP_MS: i64 = 1_700_000_000_000;
const ORIGINAL_SIZE: u64 = 1024;
const COMPRESSED_SIZE: u64 = 512;
const SHA256: [u8; 32] = [0x55; 32];
const UPLOAD_ID: [u8; 16] = [0x44; 16];

/// The contract upload metadata.
fn metadata() -> UploadMetadata {
    UploadMetadata {
        source_name: SOURCE_NAME.to_owned(),
        timestamp_ms: TIMESTAMP_MS,
        original_size: ORIGINAL_SIZE,
        compressed_size: COMPRESSED_SIZE,
        sha256: SHA256,
    }
}

/// The contract metadata with the given source name.
fn metadata_with_source_name(name: &str) -> UploadMetadata {
    UploadMetadata {
        source_name: name.to_owned(),
        ..metadata()
    }
}

/// The contract metadata as CBOR arguments.
fn metadata_args() -> Vec<Value> {
    metadata()
        .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
        .expect("contract metadata is valid")
}

/// Replaces one position of a CBOR array with `replacement`.
fn with_position(value: Value, position: usize, replacement: Value) -> Value {
    let Value::Array(mut items) = value else {
        panic!("expected a CBOR array");
    };
    items[position] = replacement;
    Value::Array(items)
}

/// Decodes a metadata value with the contract limits.
fn decode_metadata(value: &Value) -> Result<UploadMetadata, UploadError> {
    let Value::Array(items) = value else {
        panic!("upload metadata is an array");
    };
    UploadMetadata::from_args(items, SOURCE_NAME_LIMIT, SIZE_LIMIT)
}

/// `INT-NET-084/1`, `TST-034/1`.
#[test]
fn upload_metadata_matches_contract_vector() {
    let expected = {
        let mut bytes = vec![0x85, 0x69];
        bytes.extend_from_slice(SOURCE_NAME.as_bytes());
        bytes.extend_from_slice(&[0x1B, 0x00, 0x00, 0x01, 0x8B, 0xCF, 0xE5, 0x68, 0x00]);
        bytes.extend_from_slice(&[0x19, 0x04, 0x00, 0x19, 0x02, 0x00, 0x58, 0x20]);
        bytes.extend_from_slice(&SHA256);
        bytes
    };

    assert_eq!(expected.len(), 60);
    assert_eq!(cbor::encode(&Value::Array(metadata_args())), expected);

    let decoded = cbor::decode(&expected).expect("vector is valid CBOR");
    assert_eq!(decode_metadata(&decoded), Ok(metadata()));
}

/// `INT-NET-084/1`, `TST-029/1`: the success result args are `[upload_id]`.
#[test]
fn upload_result_matches_contract_vector() {
    let expected = {
        let mut bytes = vec![0x82, 0x00, 0x81, 0x50];
        bytes.extend_from_slice(&UPLOAD_ID);
        bytes
    };
    let result = UploadResult {
        upload_id: UPLOAD_ID,
    };
    let envelope = ResultEnvelope {
        code: 0,
        args: result.to_args(),
    };

    assert_eq!(expected.len(), 20);
    assert_eq!(envelope.encode(), Ok(expected.clone()));

    let decoded = ResultEnvelope::decode(&cbor::decode(&expected).expect("vector is valid CBOR"))
        .expect("envelope is valid");
    assert_eq!(UploadResult::from_args(&decoded.args), Ok(result));
}

/// `CFG-CL-032`, `TST-034/1`: the source-name limit counts UTF-8 bytes and is
/// inclusive.
#[test]
fn source_name_accepts_exactly_the_configured_limit() {
    let accepted = metadata_with_source_name(&"a".repeat(SOURCE_NAME_LIMIT));
    let args = accepted
        .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
        .expect("a 4096-byte name is allowed");
    assert_eq!(
        UploadMetadata::from_args(&args, SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Ok(accepted)
    );

    let too_long = metadata_with_source_name(&"a".repeat(SOURCE_NAME_LIMIT + 1));
    assert_eq!(
        too_long.to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Err(UploadError::SourceNameTooLong {
            actual: 4097,
            limit: 4096,
        })
    );
    let too_long_value = with_position(
        Value::Array(metadata_args()),
        0,
        Value::Text("a".repeat(SOURCE_NAME_LIMIT + 1)),
    );
    assert_eq!(
        decode_metadata(&too_long_value),
        Err(UploadError::SourceNameTooLong {
            actual: 4097,
            limit: 4096,
        })
    );

    let two_byte_name = "é".repeat(SOURCE_NAME_LIMIT / 2);
    let accepted = metadata_with_source_name(&two_byte_name);
    let args = accepted
        .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
        .expect("a 4096-byte UTF-8 name is allowed");
    assert_eq!(
        UploadMetadata::from_args(&args, SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Ok(accepted)
    );

    let overflow = metadata_with_source_name(&"é".repeat(SOURCE_NAME_LIMIT / 2 + 1));
    assert_eq!(
        overflow.to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Err(UploadError::SourceNameTooLong {
            actual: 4098,
            limit: 4096,
        })
    );
}

/// `TST-034/1`: the source name is not Unicode-normalized.
#[test]
fn source_name_round_trips_unicode_without_normalization() {
    let cyrillic = metadata_with_source_name("фото.jpg");
    let args = cyrillic
        .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
        .expect("source name is valid");
    assert_eq!(
        UploadMetadata::from_args(&args, SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Ok(cyrillic)
    );

    let composed = metadata_with_source_name("é");
    let decomposed = metadata_with_source_name("e\u{0301}");
    let composed_bytes = cbor::encode(&Value::Array(
        composed
            .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
            .expect("source name is valid"),
    ));
    let decomposed_bytes = cbor::encode(&Value::Array(
        decomposed
            .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
            .expect("source name is valid"),
    ));

    assert_ne!(composed_bytes, decomposed_bytes);
    assert_eq!(
        decode_metadata(&cbor::decode(&composed_bytes).expect("row is valid CBOR")),
        Ok(composed)
    );
    assert_eq!(
        decode_metadata(&cbor::decode(&decomposed_bytes).expect("row is valid CBOR")),
        Ok(decomposed)
    );
}

/// `CFG-CL-020`, `INT-NET-084/1`: both size fields respect the configured
/// maximum.
#[test]
fn sizes_respect_the_configured_limit() {
    let limit = 1024;
    let accepted = UploadMetadata {
        original_size: limit,
        compressed_size: limit,
        ..metadata()
    };
    let args = accepted
        .to_args(SOURCE_NAME_LIMIT, limit)
        .expect("sizes at the limit are allowed");
    assert_eq!(
        UploadMetadata::from_args(&args, SOURCE_NAME_LIMIT, limit),
        Ok(accepted)
    );

    let oversized_original = UploadMetadata {
        original_size: limit + 1,
        ..metadata()
    };
    assert_eq!(
        oversized_original.to_args(SOURCE_NAME_LIMIT, limit),
        Err(UploadError::SizeOutOfRange {
            field: "original_size",
            value: 1025,
            limit,
        })
    );

    let oversized_compressed = UploadMetadata {
        compressed_size: limit + 1,
        ..metadata()
    };
    assert_eq!(
        oversized_compressed.to_args(SOURCE_NAME_LIMIT, limit),
        Err(UploadError::SizeOutOfRange {
            field: "compressed_size",
            value: 1025,
            limit,
        })
    );

    let args = metadata_args();
    let text_original = with_position(
        Value::Array(args.clone()),
        2,
        Value::Text("1024".to_owned()),
    );
    assert_eq!(
        decode_metadata(&text_original),
        Err(UploadError::WrongType("original_size"))
    );
    let text_compressed = with_position(Value::Array(args), 3, Value::Text("512".to_owned()));
    assert_eq!(
        decode_metadata(&text_compressed),
        Err(UploadError::WrongType("compressed_size"))
    );
}

/// `INT-NET-084/1`: the timestamp is a signed 64-bit integer.
#[test]
fn timestamp_boundaries() {
    for timestamp_ms in [i64::MIN, i64::MAX] {
        let bounded = UploadMetadata {
            timestamp_ms,
            ..metadata()
        };
        let args = bounded
            .to_args(SOURCE_NAME_LIMIT, SIZE_LIMIT)
            .expect("timestamp is valid");
        assert_eq!(
            UploadMetadata::from_args(&args, SOURCE_NAME_LIMIT, SIZE_LIMIT),
            Ok(bounded)
        );
    }

    let maximum = with_position(Value::Array(metadata_args()), 1, Value::Uint(u64::MAX));
    assert_eq!(
        decode_metadata(&maximum),
        Err(UploadError::TimestampOutOfRange(u64::MAX))
    );

    let text = with_position(
        Value::Array(metadata_args()),
        1,
        Value::Text("1700000000000".to_owned()),
    );
    assert_eq!(
        decode_metadata(&text),
        Err(UploadError::WrongType("timestamp_ms"))
    );
}

/// `INT-NET-084/1`: the metadata is exactly a five-element array with fixed
/// byte-string lengths and a text source name.
#[test]
fn upload_metadata_rejects_wrong_shapes_and_lengths() {
    let items = metadata_args();
    assert_eq!(
        UploadMetadata::from_args(&items[..4], SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Err(UploadError::WrongMetadataShape)
    );
    let mut longer = items.clone();
    longer.push(Value::Null);
    assert_eq!(
        UploadMetadata::from_args(&longer, SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Err(UploadError::WrongMetadataShape)
    );
    assert_eq!(
        UploadMetadata::from_args(&[Value::Null], SOURCE_NAME_LIMIT, SIZE_LIMIT),
        Err(UploadError::WrongMetadataShape)
    );

    let short_sha256 = with_position(Value::Array(items.clone()), 4, Value::Bytes(vec![0x55; 31]));
    assert_eq!(
        decode_metadata(&short_sha256),
        Err(UploadError::InvalidLength("sha256"))
    );

    let non_text_source_name = with_position(Value::Array(items), 0, Value::Uint(1));
    assert_eq!(
        decode_metadata(&non_text_source_name),
        Err(UploadError::WrongType("source_name"))
    );
}

/// `INT-NET-084/1`, `TST-029/1`: the upload result is exactly a one-element
/// array holding a 16-byte byte string.
#[test]
fn upload_result_rejects_wrong_shapes_and_lengths() {
    assert_eq!(
        UploadResult::from_args(&[]),
        Err(UploadError::WrongResultShape)
    );
    assert_eq!(
        UploadResult::from_args(&[Value::Bytes(UPLOAD_ID.to_vec()), Value::Null]),
        Err(UploadError::WrongResultShape)
    );
    assert_eq!(
        UploadResult::from_args(&[Value::Bytes(vec![0x44; 15])]),
        Err(UploadError::InvalidLength("upload_id"))
    );
    assert_eq!(
        UploadResult::from_args(&[Value::Uint(1)]),
        Err(UploadError::WrongType("upload_id"))
    );
}
