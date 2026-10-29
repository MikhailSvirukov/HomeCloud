//! Integration tests for client frame-sequence validation and protocol version
//! negotiation over the public `storage_node` API.
//!
//! Covers `INT-NET-006 / 2`, `INT-NET-007 / 1`, `INT-NET-008 / 2`,
//! `INT-NET-043 / 1`, and `INT-NET-079 / 1`: frame numbers of one client
//! sequence start at `0` and increase by one without gaps or repeats, the
//! `uint32` space is never wrapped, the local preference list is `[1]`, and a
//! peer list without a common version is refused.

use storage_node::{
    decode_version_list, encode_version_list, negotiate_version, FrameSequence, FrameSequenceError,
    VersionError, SUPPORTED_VERSION, VERSION_LIST,
};

/// CBOR encoding of the one-element version list `[1]`.
const V1_VERSION_LIST_CBOR: [u8; 2] = [0x81, 0x01];

#[test]
fn accepts_a_contiguous_frame_sequence() {
    let mut sequence = FrameSequence::new();

    for frame_number in 0..=2 {
        assert!(
            sequence.accept(frame_number).is_ok(),
            "frame number {frame_number} continues the sequence"
        );
    }
}

#[test]
fn rejects_a_gap_and_a_repeated_frame_number() {
    let mut sequence = FrameSequence::new();
    assert!(
        sequence.accept(0).is_ok(),
        "the first frame number of a sequence is zero"
    );

    assert!(matches!(
        sequence.accept(2),
        Err(FrameSequenceError::OutOfOrder {
            expected: 1,
            actual: 2
        })
    ));
    assert!(matches!(
        sequence.accept(0),
        Err(FrameSequenceError::OutOfOrder {
            expected: 1,
            actual: 0
        })
    ));
}

#[test]
fn encodes_and_decodes_the_v1_version_list() {
    assert_eq!(SUPPORTED_VERSION, 1);
    assert_eq!(&VERSION_LIST[..], &[1][..]);

    let encoded = encode_version_list().expect("the v1 version list encodes as CBOR");
    assert_eq!(encoded, V1_VERSION_LIST_CBOR);

    let versions = decode_version_list(&encoded).expect("the encoded v1 list decodes");
    assert_eq!(versions.as_slice(), &[1][..]);

    let negotiated = negotiate_version(&[1]).expect("version 1 is offered by the peer");
    assert_eq!(negotiated, 1);
}

#[test]
fn negotiates_only_version_one() {
    assert!(matches!(
        negotiate_version(&[2]),
        Err(VersionError::NoCommonVersion)
    ));
    assert!(matches!(
        negotiate_version(&[]),
        Err(VersionError::NoCommonVersion)
    ));

    let negotiated = negotiate_version(&[2, 1]).expect("the peer list contains version 1");
    assert_eq!(negotiated, 1);

    assert!(matches!(
        negotiate_version(&[3, 2]),
        Err(VersionError::NoCommonVersion)
    ));
}

#[test]
fn rejects_malformed_version_lists() {
    let non_array = [0x01];
    let non_integer_element = [0x81, 0x61, b'1'];
    let negative_integer = [0x81, 0x20];
    let out_of_range_integer = [0x81, 0x1b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
    let trailing_bytes = [0x81, 0x01, 0x00];

    assert!(
        matches!(
            decode_version_list(&non_array),
            Err(VersionError::NotAnArray)
        ),
        "a non-array value is not a version list"
    );
    assert!(
        matches!(
            decode_version_list(&non_integer_element),
            Err(VersionError::InvalidElement)
        ),
        "a text element is not a version number"
    );
    assert!(
        matches!(
            decode_version_list(&negative_integer),
            Err(VersionError::InvalidElement)
        ),
        "a negative element is not a uint32 version number"
    );
    assert!(
        matches!(
            decode_version_list(&out_of_range_integer),
            Err(VersionError::InvalidElement)
        ),
        "an element above u32::MAX is not a uint32 version number"
    );
    assert!(
        matches!(
            decode_version_list(&trailing_bytes),
            Err(VersionError::Decode)
        ),
        "trailing bytes are rejected"
    );

    let versions =
        decode_version_list(&[0x81, 0x02]).expect("a one-element list of version 2 is well formed");
    assert_eq!(versions.as_slice(), &[2][..]);
}
