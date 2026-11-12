//! Deterministic CBOR subset used by the fixed v1 wire structures.
//!
//! The v1 headers and the result envelope need only unsigned integers,
//! negative integers, byte strings, text strings, `null`, and arrays. Encoding
//! follows RFC 8949 deterministic encoding with definite lengths; maps, tags,
//! indefinite lengths, and simple values other than `null` are rejected on
//! decode.

const MAJOR_UNSIGNED: u8 = 0;
const MAJOR_NEGATIVE: u8 = 1;
const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;
const MAJOR_SIMPLE: u8 = 7;

const SIMPLE_NULL: u8 = 22;
const SIMPLE_ONE_BYTE: u8 = 24;

const MAX_SHORT_ARGUMENT: u64 = 23;
const MIN_ONE_BYTE_ARGUMENT: u8 = 24;
const MIN_TWO_BYTE_ARGUMENT: u16 = 0x100;
const MIN_FOUR_BYTE_ARGUMENT: u32 = 0x1_0000;
const MIN_EIGHT_BYTE_ARGUMENT: u64 = 0x1_0000_0000;
const MAX_PREALLOCATED_ITEMS: usize = 64;
const MAX_NESTING_DEPTH: usize = 64;

/// A CBOR value used by the fixed wire structures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A non-negative integer (major type 0).
    Uint(u64),
    /// A negative integer (major type 1); non-negative integers use [`Value::Uint`].
    Int(i64),
    /// A definite-length byte string.
    Bytes(Vec<u8>),
    /// A definite-length UTF-8 text string.
    Text(String),
    /// The CBOR `null` value.
    Null,
    /// A definite-length array.
    Array(Vec<Value>),
}

/// Reasons a CBOR value cannot be decoded by this subset.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum CborError {
    /// The input ended before the value was complete.
    #[error("CBOR input ended before the value was complete")]
    Truncated,
    /// The input nests arrays deeper than the supported limit.
    #[error("CBOR nesting exceeds the supported depth")]
    NestingTooDeep,
    /// The input contained bytes after the first value.
    #[error("CBOR input has trailing bytes after the value")]
    TrailingBytes,
    /// The major type is not supported by the fixed wire structures.
    #[error("unsupported CBOR major type {0}")]
    UnsupportedMajorType(u8),
    /// The simple value is not supported; only `null` is represented.
    #[error("unsupported CBOR simple value {0}")]
    UnsupportedSimpleValue(u8),
    /// An integer used more bytes than its minimal encoding.
    #[error("CBOR integer is not minimally encoded")]
    NonMinimalInteger,
    /// An integer does not fit the target numeric type.
    #[error("CBOR integer does not fit the target range")]
    IntegerOverflow,
    /// A text string is not valid UTF-8.
    #[error("CBOR text is not valid UTF-8")]
    InvalidUtf8,
}

/// Deterministic (RFC 8949) minimal-length encoding with definite lengths.
///
/// A non-negative [`Value::Int`] is encoded through major type 0, which is the
/// same byte sequence as the corresponding [`Value::Uint`].
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

/// Decodes exactly one value; rejects trailing bytes, indefinite lengths, maps,
/// tags, unsupported simple values, non-minimal integer encodings, and invalid
/// UTF-8.
pub fn decode(bytes: &[u8]) -> Result<Value, CborError> {
    let (value, consumed) = decode_prefix(bytes)?;
    if consumed == bytes.len() {
        Ok(value)
    } else {
        Err(CborError::TrailingBytes)
    }
}

/// Same as [`decode`], returning the number of consumed bytes.
pub fn decode_prefix(bytes: &[u8]) -> Result<(Value, usize), CborError> {
    decode_value(bytes, 0)
}

fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Uint(number) => encode_argument(MAJOR_UNSIGNED, *number, out),
        Value::Int(number) => match u64::try_from(*number) {
            Ok(non_negative) => encode_argument(MAJOR_UNSIGNED, non_negative, out),
            Err(_) => {
                let magnitude = number.unsigned_abs() - 1;
                encode_argument(MAJOR_NEGATIVE, magnitude, out);
            }
        },
        Value::Bytes(bytes) => {
            encode_argument(MAJOR_BYTES, bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        Value::Text(text) => {
            encode_argument(MAJOR_TEXT, text.len() as u64, out);
            out.extend_from_slice(text.as_bytes());
        }
        Value::Null => encode_argument(MAJOR_SIMPLE, u64::from(SIMPLE_NULL), out),
        Value::Array(items) => {
            encode_argument(MAJOR_ARRAY, items.len() as u64, out);
            for item in items {
                encode_into(item, out);
            }
        }
    }
}

fn encode_argument(major: u8, argument: u64, out: &mut Vec<u8>) {
    let major_bits = major << 5;
    match argument {
        0..=MAX_SHORT_ARGUMENT => out.push(major_bits | (argument as u8)),
        24..=0xFF => {
            out.push(major_bits | SIMPLE_ONE_BYTE);
            out.push(argument as u8);
        }
        0x100..=0xFFFF => {
            out.push(major_bits | 25);
            out.extend_from_slice(&(argument as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(major_bits | 26);
            out.extend_from_slice(&(argument as u32).to_be_bytes());
        }
        _ => {
            out.push(major_bits | 27);
            out.extend_from_slice(&argument.to_be_bytes());
        }
    }
}

fn decode_value(bytes: &[u8], depth: usize) -> Result<(Value, usize), CborError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(CborError::NestingTooDeep);
    }

    let (&first, rest) = bytes.split_first().ok_or(CborError::Truncated)?;
    let major = first >> 5;
    let additional = first & 0x1F;

    match major {
        MAJOR_UNSIGNED => {
            let (argument, used) = read_argument(major, additional, rest)?;
            Ok((Value::Uint(argument), 1 + used))
        }
        MAJOR_NEGATIVE => {
            let (argument, used) = read_argument(major, additional, rest)?;
            let magnitude = i64::try_from(argument).map_err(|_| CborError::IntegerOverflow)?;
            Ok((Value::Int(-1 - magnitude), 1 + used))
        }
        MAJOR_BYTES => {
            let (argument, used) = read_argument(major, additional, rest)?;
            let (data, consumed) = take_bytes(rest, used, argument)?;
            Ok((Value::Bytes(data.to_vec()), 1 + consumed))
        }
        MAJOR_TEXT => {
            let (argument, used) = read_argument(major, additional, rest)?;
            let (data, consumed) = take_bytes(rest, used, argument)?;
            let text = String::from_utf8(data.to_vec()).map_err(|_| CborError::InvalidUtf8)?;
            Ok((Value::Text(text), 1 + consumed))
        }
        MAJOR_ARRAY => {
            let (argument, used) = read_argument(major, additional, rest)?;
            let length = usize::try_from(argument).map_err(|_| CborError::IntegerOverflow)?;
            let mut items = Vec::with_capacity(length.min(MAX_PREALLOCATED_ITEMS));
            let mut cursor = used;
            for _ in 0..length {
                let remaining = rest.get(cursor..).ok_or(CborError::Truncated)?;
                let (item, consumed) = decode_value(remaining, depth + 1)?;
                items.push(item);
                cursor = cursor
                    .checked_add(consumed)
                    .ok_or(CborError::IntegerOverflow)?;
            }
            Ok((Value::Array(items), 1 + cursor))
        }
        MAJOR_MAP | MAJOR_TAG => Err(CborError::UnsupportedMajorType(major)),
        MAJOR_SIMPLE => decode_simple(additional, rest),
        _ => Err(CborError::UnsupportedMajorType(major)),
    }
}

fn decode_simple(additional: u8, rest: &[u8]) -> Result<(Value, usize), CborError> {
    match additional {
        SIMPLE_NULL => Ok((Value::Null, 1)),
        SIMPLE_ONE_BYTE => {
            let [simple] = take::<1>(rest)?;
            Err(CborError::UnsupportedSimpleValue(simple))
        }
        _ => Err(CborError::UnsupportedSimpleValue(additional)),
    }
}

fn read_argument(major: u8, additional: u8, rest: &[u8]) -> Result<(u64, usize), CborError> {
    match additional {
        0..=23 => Ok((u64::from(additional), 0)),
        24 => {
            let [value] = take::<1>(rest)?;
            if value < MIN_ONE_BYTE_ARGUMENT {
                Err(CborError::NonMinimalInteger)
            } else {
                Ok((u64::from(value), 1))
            }
        }
        25 => {
            let value = u16::from_be_bytes(take::<2>(rest)?);
            if value < MIN_TWO_BYTE_ARGUMENT {
                Err(CborError::NonMinimalInteger)
            } else {
                Ok((u64::from(value), 2))
            }
        }
        26 => {
            let value = u32::from_be_bytes(take::<4>(rest)?);
            if value < MIN_FOUR_BYTE_ARGUMENT {
                Err(CborError::NonMinimalInteger)
            } else {
                Ok((u64::from(value), 4))
            }
        }
        27 => {
            let value = u64::from_be_bytes(take::<8>(rest)?);
            if value < MIN_EIGHT_BYTE_ARGUMENT {
                Err(CborError::NonMinimalInteger)
            } else {
                Ok((value, 8))
            }
        }
        _ => Err(CborError::UnsupportedMajorType(major)),
    }
}

fn take_bytes(rest: &[u8], offset: usize, length: u64) -> Result<(&[u8], usize), CborError> {
    let length = usize::try_from(length).map_err(|_| CborError::IntegerOverflow)?;
    let end = offset
        .checked_add(length)
        .ok_or(CborError::IntegerOverflow)?;
    let data = rest.get(offset..end).ok_or(CborError::Truncated)?;
    Ok((data, end))
}

fn take<const N: usize>(bytes: &[u8]) -> Result<[u8; N], CborError> {
    let slice = bytes.get(..N).ok_or(CborError::Truncated)?;
    <[u8; N]>::try_from(slice).map_err(|_| CborError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::{CborError, Value, decode, decode_prefix, encode};

    #[test]
    fn encodes_minimal_unsigned_integers_at_width_boundaries() {
        assert_eq!(encode(&Value::Uint(23)), vec![0x17]);
        assert_eq!(encode(&Value::Uint(24)), vec![0x18, 0x18]);
        assert_eq!(encode(&Value::Uint(255)), vec![0x18, 0xFF]);
        assert_eq!(encode(&Value::Uint(256)), vec![0x19, 0x01, 0x00]);
        assert_eq!(encode(&Value::Uint(65_535)), vec![0x19, 0xFF, 0xFF]);
        assert_eq!(
            encode(&Value::Uint(65_536)),
            vec![0x1A, 0x00, 0x01, 0x00, 0x00]
        );
        assert_eq!(
            encode(&Value::Uint(1 << 32)),
            vec![0x1B, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn encodes_i64_min_with_eight_byte_argument() {
        assert_eq!(
            encode(&Value::Int(i64::MIN)),
            vec![0x3B, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn encodes_negative_integers_with_minimal_width() {
        assert_eq!(encode(&Value::Int(-1)), vec![0x20]);
        assert_eq!(encode(&Value::Int(-24)), vec![0x37]);
        assert_eq!(encode(&Value::Int(-25)), vec![0x38, 0x18]);
    }

    #[test]
    fn encodes_non_negative_int_as_unsigned() {
        assert_eq!(encode(&Value::Int(1)), vec![0x01]);
    }

    #[test]
    fn round_trips_negative_integers() {
        assert_eq!(decode(&[0x38, 0x18]), Ok(Value::Int(-25)));
        assert_eq!(
            decode(&[0x3B, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
            Ok(Value::Int(i64::MIN))
        );
    }

    #[test]
    fn decode_rejects_non_minimal_integers() {
        assert_eq!(decode(&[0x18, 0x17]), Err(CborError::NonMinimalInteger));
        assert_eq!(
            decode(&[0x19, 0x00, 0xFF]),
            Err(CborError::NonMinimalInteger)
        );
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        assert_eq!(decode(&[0x00, 0x00]), Err(CborError::TrailingBytes));
    }

    #[test]
    fn decode_rejects_maps_and_tags() {
        assert_eq!(decode(&[0xA0]), Err(CborError::UnsupportedMajorType(5)));
        assert_eq!(
            decode(&[0xC0, 0x00]),
            Err(CborError::UnsupportedMajorType(6))
        );
    }

    #[test]
    fn decode_rejects_indefinite_length_array() {
        assert_eq!(decode(&[0x9F]), Err(CborError::UnsupportedMajorType(4)));
    }

    #[test]
    fn decode_rejects_indefinite_length_bytes() {
        assert_eq!(decode(&[0x5F]), Err(CborError::UnsupportedMajorType(2)));
    }

    #[test]
    fn decode_rejects_unsupported_simple_value() {
        assert_eq!(decode(&[0xF7]), Err(CborError::UnsupportedSimpleValue(23)));
    }

    #[test]
    fn decode_rejects_invalid_utf8_text() {
        assert_eq!(decode(&[0x61, 0xFF]), Err(CborError::InvalidUtf8));
    }

    #[test]
    fn decode_rejects_truncated_integer() {
        assert_eq!(decode(&[0x18]), Err(CborError::Truncated));
    }

    #[test]
    fn decode_rejects_nesting_beyond_limit() {
        let bytes = vec![0x81; 70];
        assert_eq!(decode(&bytes), Err(CborError::NestingTooDeep));
    }

    #[test]
    fn decode_prefix_reports_consumed_bytes() {
        assert_eq!(decode_prefix(&[0x01, 0x02]), Ok((Value::Uint(1), 1)));
    }
}
