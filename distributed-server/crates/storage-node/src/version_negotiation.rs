//! Protocol version negotiation.
//!
//! SRS: `INT-NET-006 / 2`, `INT-NET-007 / 1`, `INT-NET-043 / 1`, `INT-NET-079 / 1`.

use ciborium::value::Value;

/// Protocol version supported by every v1 implementation.
pub const SUPPORTED_VERSION: u32 = 1;

/// The complete v1 version advertisement list.
pub const VERSION_LIST: [u32; 1] = [SUPPORTED_VERSION];

/// Failures from encoding, decoding, or negotiating a version list.
#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    /// The decoded value is not a CBOR array.
    #[error("the version list is not a CBOR array")]
    NotAnArray,
    /// A version entry is not a uint32.
    #[error("a version entry is not a uint32")]
    InvalidElement,
    /// The version list could not be decoded, or trailing bytes followed it.
    #[error("the version list could not be decoded")]
    Decode,
    /// The version list could not be encoded.
    #[error("the version list could not be encoded")]
    Encode,
    /// The peer and local version lists have no common version.
    #[error("the peer and local version lists have no common version")]
    NoCommonVersion,
}

/// Encodes the v1 advertisement list `[1]` as CBOR.
pub fn encode_version_list() -> Result<Vec<u8>, VersionError> {
    let value = Value::Array(vec![Value::Integer(SUPPORTED_VERSION.into())]);
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes).map_err(|_| VersionError::Encode)?;
    Ok(bytes)
}

/// Decodes a CBOR version list and rejects trailing bytes.
pub fn decode_version_list(bytes: &[u8]) -> Result<Vec<u32>, VersionError> {
    let mut slice = bytes;
    let value: Value = ciborium::de::from_reader(&mut slice).map_err(|_| VersionError::Decode)?;
    if !slice.is_empty() {
        return Err(VersionError::Decode);
    }

    let Value::Array(elements) = value else {
        return Err(VersionError::NotAnArray);
    };

    elements
        .into_iter()
        .map(|element| match element {
            Value::Integer(version) => {
                u32::try_from(version).map_err(|_| VersionError::InvalidElement)
            }
            _ => Err(VersionError::InvalidElement),
        })
        .collect()
}

/// Selects version `1` when the peer advertises it.
pub fn negotiate_version(peer_versions: &[u32]) -> Result<u32, VersionError> {
    if !peer_versions.contains(&SUPPORTED_VERSION) {
        return Err(VersionError::NoCommonVersion);
    }

    Ok(SUPPORTED_VERSION)
}
