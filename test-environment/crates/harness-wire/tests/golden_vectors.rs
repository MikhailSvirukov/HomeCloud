//! Golden byte vectors for the published v1 wire contract.
//!
//! Every test pins exact bytes from [`contracts/wire-v1.md`] so component
//! implementations can consume these vectors as external conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::cbor::{self, CborError, Value};
use harness_wire::frame::{self, Frame, FrameError};
use harness_wire::header::{ExternalFrameHeader, HeaderError};
use harness_wire::result::{
    AUTHENTICATION_REJECTED, BLOB_QUORUM_UNAVAILABLE, CODE_SUCCESS, EXPIRED_SNAPSHOT,
    FILE_MISMATCH, FILE_SIZE_EXCEEDED, INTERNAL_ERROR, INVALID_ARGUMENTS, INVALID_CURSOR,
    INVALID_FRAME_SEQUENCE, INVALID_REQUEST_STATE, RAFT_QUORUM_UNAVAILABLE, REGISTRATION_CONFLICT,
    REGISTRATION_REJECTED, REPEATED_REQUEST_ID, ResultClass, ResultEnvelope, ResultError,
    STORAGE_CAPACITY_EXHAUSTED, TEMPORARY_UNAVAILABLE, UNKNOWN_OPERATION, UPLOAD_ID_CONFLICT,
    class_of,
};

/// Sender identifier of the contract's ordinary header vector.
const SENDER_ID: [u8; 16] = [0xAA; 16];

/// Request identifier of the contract's ordinary header vector.
const REQUEST_ID: [u8; 16] = [0xBB; 16];

/// Request identifier of the contract's primary-registration vector.
const PRIMARY_REQUEST_ID: [u8; 16] = [0x33; 16];

/// Bootstrap signing key of the contract's primary-registration vector.
const BOOTSTRAP_SIGNING_PUBLIC_KEY: [u8; 32] = [0x55; 32];

/// Signature bytes of the contract's frame vector.
const SIGNATURE: [u8; 64] = [0xCC; 64];

/// Payload bytes of the contract's frame vector.
const PAYLOAD: [u8; 2] = [0xDE, 0xAD];

/// `CFG-CL-031` label limit used by the contract vectors.
const LABEL_LIMIT: usize = 256;

/// `CFG-CL-011` frame body limit used by the contract vectors.
const FRAME_LIMIT: u32 = 4_194_304;

/// Offset of `frame_number` inside the encoded ordinary header.
const FRAME_NUMBER_OFFSET: usize = 36;

/// Maps each published result code to its exact contract bytes.
const PUBLISHED_CODES: [(u32, [u8; 7]); 18] = [
    (
        UNKNOWN_OPERATION,
        [0x82, 0x1A, 0x00, 0x01, 0x00, 0x01, 0x80],
    ),
    (
        INVALID_ARGUMENTS,
        [0x82, 0x1A, 0x00, 0x01, 0x00, 0x02, 0x80],
    ),
    (
        INVALID_REQUEST_STATE,
        [0x82, 0x1A, 0x00, 0x01, 0x00, 0x03, 0x80],
    ),
    (
        REPEATED_REQUEST_ID,
        [0x82, 0x1A, 0x00, 0x01, 0x00, 0x04, 0x80],
    ),
    (
        AUTHENTICATION_REJECTED,
        [0x82, 0x1A, 0x00, 0x02, 0x00, 0x01, 0x80],
    ),
    (
        REGISTRATION_CONFLICT,
        [0x82, 0x1A, 0x00, 0x03, 0x00, 0x01, 0x80],
    ),
    (
        REGISTRATION_REJECTED,
        [0x82, 0x1A, 0x00, 0x03, 0x00, 0x02, 0x80],
    ),
    (INVALID_CURSOR, [0x82, 0x1A, 0x00, 0x04, 0x00, 0x01, 0x80]),
    (EXPIRED_SNAPSHOT, [0x82, 0x1A, 0x00, 0x04, 0x00, 0x02, 0x80]),
    (
        FILE_SIZE_EXCEEDED,
        [0x82, 0x1A, 0x00, 0x05, 0x00, 0x01, 0x80],
    ),
    (FILE_MISMATCH, [0x82, 0x1A, 0x00, 0x05, 0x00, 0x02, 0x80]),
    (
        INVALID_FRAME_SEQUENCE,
        [0x82, 0x1A, 0x00, 0x05, 0x00, 0x03, 0x80],
    ),
    (
        UPLOAD_ID_CONFLICT,
        [0x82, 0x1A, 0x00, 0x05, 0x00, 0x04, 0x80],
    ),
    (
        RAFT_QUORUM_UNAVAILABLE,
        [0x82, 0x1A, 0x00, 0x06, 0x00, 0x01, 0x80],
    ),
    (
        BLOB_QUORUM_UNAVAILABLE,
        [0x82, 0x1A, 0x00, 0x06, 0x00, 0x02, 0x80],
    ),
    (
        TEMPORARY_UNAVAILABLE,
        [0x82, 0x1A, 0x00, 0x07, 0x00, 0x01, 0x80],
    ),
    (
        STORAGE_CAPACITY_EXHAUSTED,
        [0x82, 0x1A, 0x00, 0x07, 0x00, 0x02, 0x80],
    ),
    (INTERNAL_ERROR, [0x82, 0x1A, 0x00, 0x08, 0x00, 0x01, 0x80]),
];

/// Maps each configured result-code range boundary to its reserved class.
const CLASS_BOUNDARIES: [(u32, ResultClass); 13] = [
    (CODE_SUCCESS, ResultClass::Success),
    (0x0001_0000, ResultClass::Request),
    (0x0001_FFFF, ResultClass::Request),
    (0x0002_0000, ResultClass::Authentication),
    (0x0003_0000, ResultClass::Registration),
    (0x0004_0000, ResultClass::Catalog),
    (0x0005_0000, ResultClass::FileUpload),
    (0x0006_0000, ResultClass::Quorum),
    (0x0007_0000, ResultClass::Resource),
    (0x0008_0000, ResultClass::Internal),
    (0x0008_FFFF, ResultClass::Internal),
    (0x0009_0000, ResultClass::Reserved),
    (u32::MAX, ResultClass::Reserved),
];

/// The ordinary seven-position header of the contract vectors.
fn ordinary_header() -> ExternalFrameHeader {
    ExternalFrameHeader {
        sender_id: Some(SENDER_ID),
        request_id: REQUEST_ID,
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    }
}

/// The primary-registration seven-position header of the contract vectors.
fn primary_registration_header() -> ExternalFrameHeader {
    ExternalFrameHeader {
        sender_id: None,
        request_id: PRIMARY_REQUEST_ID,
        frame_number: 7,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: Some(BOOTSTRAP_SIGNING_PUBLIC_KEY),
    }
}

/// Builds the seven-position header array for decode inputs.
fn header_value(
    sender_id: Option<[u8; 16]>,
    request_id: [u8; 16],
    frame_number: u64,
    upload_id: Option<[u8; 16]>,
    responder_label: Option<&str>,
    bootstrap_signing_public_key: Option<[u8; 32]>,
) -> Value {
    Value::Array(vec![
        Value::Uint(1),
        optional_bytes(sender_id),
        Value::Bytes(request_id.to_vec()),
        Value::Uint(frame_number),
        optional_bytes(upload_id),
        responder_label.map_or(Value::Null, |label| Value::Text(label.to_owned())),
        optional_bytes(bootstrap_signing_public_key),
    ])
}

/// The ordinary header array with `label` as its responder label.
fn header_with_label(label: &str) -> Value {
    header_value(Some(SENDER_ID), REQUEST_ID, 0, None, Some(label), None)
}

fn optional_bytes<const N: usize>(bytes: Option<[u8; N]>) -> Value {
    bytes.map_or(Value::Null, |bytes| Value::Bytes(bytes.to_vec()))
}

/// Literal bytes of the ordinary header up to the `frame_number` position.
fn expected_ordinary_prefix() -> Vec<u8> {
    let mut bytes = vec![0x87, 0x01, 0x50];
    bytes.extend_from_slice(&SENDER_ID);
    bytes.push(0x50);
    bytes.extend_from_slice(&REQUEST_ID);
    bytes
}

/// The exact encoded ordinary header of the contract vectors.
fn contract_header() -> Vec<u8> {
    ordinary_header()
        .encode(LABEL_LIMIT)
        .expect("contract header is valid")
}

/// The exact frame of the contract's frame-body vector.
fn contract_frame() -> Frame {
    Frame {
        header: contract_header(),
        signature: SIGNATURE,
        payload: PAYLOAD.to_vec(),
    }
}

/// Decodes the responder label from encoded header bytes.
fn decoded_label(bytes: &[u8]) -> Option<String> {
    let value = cbor::decode(bytes).expect("header bytes are valid CBOR");
    ExternalFrameHeader::decode(&value, LABEL_LIMIT)
        .expect("header bytes are valid")
        .responder_label
}

/// Golden vector: `INT-NET-006/2`, `INT-NET-047/2`, `INT-NET-049/1`,
/// `INT-NET-050/4`, `TST-030/1`.
#[test]
fn ordinary_external_header_matches_contract_vector() {
    let header = ordinary_header();

    let mut expected = expected_ordinary_prefix();
    expected.extend_from_slice(&[0x00, 0xF6, 0xF6, 0xF6]);

    assert_eq!(expected.len(), 40);
    assert_eq!(header.encode(LABEL_LIMIT), Ok(expected.clone()));

    let value = cbor::decode(&expected).expect("contract vector is valid CBOR");
    assert_eq!(ExternalFrameHeader::decode(&value, LABEL_LIMIT), Ok(header));
}

/// Golden vector: `INT-NET-047/2`, `INT-NET-050/4` (UTF-8 label position).
#[test]
fn ordinary_external_header_with_label_matches_contract_vector() {
    let header = ExternalFrameHeader {
        responder_label: Some("node-1".to_owned()),
        ..ordinary_header()
    };

    let mut expected = expected_ordinary_prefix();
    expected.extend_from_slice(&[0x00, 0xF6, 0x66]);
    expected.extend_from_slice(b"node-1");
    expected.push(0xF6);

    assert_eq!(header.encode(LABEL_LIMIT), Ok(expected.clone()));

    let value = cbor::decode(&expected).expect("contract vector is valid CBOR");
    assert_eq!(ExternalFrameHeader::decode(&value, LABEL_LIMIT), Ok(header));
}

/// Golden vector: `INT-NET-008/2`, `INT-NET-050/4` (primary registration),
/// `INT-NET-081/1`.
#[test]
fn primary_registration_external_header_matches_contract_vector() {
    let header = primary_registration_header();

    let mut expected = vec![0x87, 0x01, 0xF6, 0x50];
    expected.extend_from_slice(&PRIMARY_REQUEST_ID);
    expected.extend_from_slice(&[0x07, 0xF6, 0xF6, 0x58, 0x20]);
    expected.extend_from_slice(&BOOTSTRAP_SIGNING_PUBLIC_KEY);

    assert_eq!(expected.len(), 57);
    assert_eq!(header.encode(LABEL_LIMIT), Ok(expected.clone()));

    let value = cbor::decode(&expected).expect("contract vector is valid CBOR");
    assert_eq!(ExternalFrameHeader::decode(&value, LABEL_LIMIT), Ok(header));
}

/// `INT-NET-005/1`, `INT-NET-006/2`, `TST-030/1`: the version is position one
/// and must use the minimal integer encoding.
#[test]
fn version_position_uses_minimal_uint() {
    let encoded = contract_header();
    assert_eq!(&encoded[..2], &[0x87, 0x01]);

    let mut non_minimal = vec![0x87, 0x18, 0x01];
    non_minimal.extend_from_slice(&encoded[2..]);
    assert_eq!(
        cbor::decode(&non_minimal),
        Err(CborError::NonMinimalInteger)
    );
}

/// `INT-NET-006/2`, `INT-NET-050/4`, `TST-032/1`: `frame_number` switches at
/// every `uint32` width boundary and never uses a wider encoding.
#[test]
fn frame_number_uses_minimal_uint32_widths() {
    const ENCODINGS: [(u32, &[u8]); 8] = [
        (0, &[0x00]),
        (23, &[0x17]),
        (24, &[0x18, 0x18]),
        (255, &[0x18, 0xFF]),
        (256, &[0x19, 0x01, 0x00]),
        (65_535, &[0x19, 0xFF, 0xFF]),
        (65_536, &[0x1A, 0x00, 0x01, 0x00, 0x00]),
        (u32::MAX, &[0x1A, 0xFF, 0xFF, 0xFF, 0xFF]),
    ];

    for (frame_number, expected) in ENCODINGS {
        let header = ExternalFrameHeader {
            frame_number,
            ..ordinary_header()
        };
        let encoded = header.encode(LABEL_LIMIT).expect("header is valid");
        let end = FRAME_NUMBER_OFFSET + expected.len();
        assert_eq!(&encoded[FRAME_NUMBER_OFFSET..end], expected);
    }
}

/// `INT-NET-008/2`, `INT-NET-050/4`, `TST-032/1`: a frame number above
/// `uint32::MAX` is rejected instead of wrapping.
#[test]
fn frame_number_above_uint32_is_rejected() {
    let value = header_value(Some(SENDER_ID), REQUEST_ID, 0x1_0000_0000, None, None, None);

    assert_eq!(
        ExternalFrameHeader::decode(&value, LABEL_LIMIT),
        Err(HeaderError::FrameNumberOutOfRange(0x1_0000_0000))
    );
}

/// `INT-NET-050/4`, `CFG-CL-031/1`: the responder-label limit counts UTF-8
/// bytes, not characters.
#[test]
fn responder_label_accepts_exactly_the_configured_limit() {
    let accepted = "a".repeat(LABEL_LIMIT);
    let decoded = ExternalFrameHeader::decode(&header_with_label(&accepted), LABEL_LIMIT)
        .expect("a 256-byte label is allowed");
    assert_eq!(decoded.responder_label.as_deref(), Some(accepted.as_str()));

    let too_long = "a".repeat(LABEL_LIMIT + 1);
    assert_eq!(
        ExternalFrameHeader::decode(&header_with_label(&too_long), LABEL_LIMIT),
        Err(HeaderError::LabelTooLong {
            actual: 257,
            limit: 256,
        })
    );

    let two_byte_label = "é".repeat(LABEL_LIMIT / 2);
    assert_eq!(two_byte_label.len(), LABEL_LIMIT);
    let expected = ExternalFrameHeader {
        responder_label: Some(two_byte_label.clone()),
        ..ordinary_header()
    };
    assert_eq!(
        ExternalFrameHeader::decode(&header_with_label(&two_byte_label), LABEL_LIMIT),
        Ok(expected)
    );

    let two_byte_overflow = "é".repeat(LABEL_LIMIT / 2 + 1);
    assert_eq!(
        ExternalFrameHeader::decode(&header_with_label(&two_byte_overflow), LABEL_LIMIT),
        Err(HeaderError::LabelTooLong {
            actual: 258,
            limit: 256,
        })
    );
}

/// `INT-NET-050/4`: the label is not Unicode-normalized on encode or decode.
#[test]
fn responder_label_round_trips_unicode_without_normalization() {
    let composed = ExternalFrameHeader {
        responder_label: Some("é".to_owned()),
        ..ordinary_header()
    };
    let decomposed = ExternalFrameHeader {
        responder_label: Some("e\u{0301}".to_owned()),
        ..ordinary_header()
    };

    let composed_bytes = composed.encode(LABEL_LIMIT).expect("header is valid");
    let decomposed_bytes = decomposed.encode(LABEL_LIMIT).expect("header is valid");

    assert_ne!(composed_bytes, decomposed_bytes);
    assert_eq!(decoded_label(&composed_bytes), Some("é".to_owned()));
    assert_eq!(
        decoded_label(&decomposed_bytes),
        Some("e\u{0301}".to_owned())
    );
}

/// `INT-NET-002/1`, `INT-NET-003/1`, `INT-NET-081/1`.
#[test]
fn frame_body_matches_contract_vector() {
    let frame = contract_frame();

    let mut expected = vec![0x00, 0x00, 0x00, 0x6A];
    expected.extend_from_slice(&frame.header);
    expected.extend_from_slice(&SIGNATURE);
    expected.extend_from_slice(&PAYLOAD);

    assert_eq!(expected.len(), 110);
    assert_eq!(frame::encode(&frame, FRAME_LIMIT), Ok(expected.clone()));
    assert_eq!(
        frame::decode(&expected, FRAME_LIMIT),
        Ok((frame, expected.len()))
    );
}

/// `INT-NET-019/1`, `INT-NET-081/1`: the signed range is the length prefix, the
/// header, and the payload, without the signature.
#[test]
fn signed_message_matches_contract_signature_range() {
    let frame = contract_frame();

    let mut expected = vec![0x00, 0x00, 0x00, 0x6A];
    expected.extend_from_slice(&frame.header);
    expected.extend_from_slice(&PAYLOAD);

    assert_eq!(expected.len(), 46);
    assert_eq!(frame::signed_message(&frame, FRAME_LIMIT), Ok(expected));
}

/// `INT-NET-003/1`, `INT-NET-004/1`, `CFG-CL-011`: the body limit is inclusive
/// and counts header, signature, and payload.
#[test]
fn frame_body_limit_boundaries() {
    let at_limit = Frame {
        header: contract_header(),
        signature: SIGNATURE,
        payload: vec![0x00; 3992],
    };
    let encoded = frame::encode(&at_limit, 4096).expect("a body of exactly 4096 bytes is accepted");
    assert_eq!(encoded.len(), 4 + 4096);

    let over_limit = Frame {
        header: contract_header(),
        signature: SIGNATURE,
        payload: vec![0x00; 3993],
    };
    assert_eq!(
        frame::encode(&over_limit, 4096),
        Err(FrameError::BodyTooLarge {
            declared: 4097,
            limit: 4096,
        })
    );
}

/// `INT-NET-061/1`, `INT-NET-062/1`: a success envelope is `[0, []]`.
#[test]
fn result_envelope_success_vector() {
    let envelope = ResultEnvelope {
        code: CODE_SUCCESS,
        args: vec![],
    };

    assert_eq!(envelope.encode(), Ok(vec![0x82, 0x00, 0x80]));
}

/// `INT-NET-062/1`, `INT-NET-063/1`, `INT-NET-069/1`, `TST-019/1`,
/// `TST-022/1`: every published code encodes to its contract bytes with an
/// empty argument array and decodes back unchanged.
#[test]
fn published_result_codes_match_contract_vectors() {
    for (code, expected) in PUBLISHED_CODES {
        let envelope = ResultEnvelope { code, args: vec![] };

        assert_eq!(
            envelope.encode(),
            Ok(expected.to_vec()),
            "code {code:#010X}"
        );

        let value = cbor::decode(&expected).expect("contract vector is valid CBOR");
        assert_eq!(
            ResultEnvelope::decode(&value),
            Ok(envelope),
            "code {code:#010X}"
        );
    }
}

/// `INT-NET-063/1`, `INT-NET-064/1`: the reserved range boundaries map to
/// their classes.
#[test]
fn result_code_classes_match_reserved_ranges() {
    for (code, class) in CLASS_BOUNDARIES {
        assert_eq!(class_of(code), class, "code {code:#010X}");
    }
}

/// `INT-NET-069/1`, `TST-022/1`: an error envelope is `[code, []]` and a
/// non-empty error argument array is rejected.
#[test]
fn error_envelope_golden_vector() {
    let envelope = ResultEnvelope {
        code: FILE_SIZE_EXCEEDED,
        args: vec![],
    };

    assert_eq!(
        envelope.encode(),
        Ok(vec![0x82, 0x1A, 0x00, 0x05, 0x00, 0x01, 0x80])
    );

    let with_diagnostics = Value::Array(vec![
        Value::Uint(u64::from(FILE_SIZE_EXCEEDED)),
        Value::Array(vec![Value::Null]),
    ]);
    assert_eq!(
        ResultEnvelope::decode(&with_diagnostics),
        Err(ResultError::NonEmptyErrorArgs)
    );
}
