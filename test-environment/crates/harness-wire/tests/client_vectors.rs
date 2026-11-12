//! Golden vectors for the v1 authenticated client logical header.
//!
//! Every test pins the bytes fixed by [`contracts/wire-v1.md`] §5 so component
//! implementations can consume these vectors as external conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::catalog::CatalogPageRequest;
use harness_wire::cbor::{self, Value};
use harness_wire::client::{ClientError, ClientRequest, CredentialLimits, OperationType};

/// Login of the contract vectors.
const LOGIN: &str = "alice";
/// Password of the contract vectors.
const PASSWORD: &str = "password123";
/// Published v1 credential limits (`CFG-CL-029`).
const LIMITS: CredentialLimits = CredentialLimits::V1_DEFAULT;

/// The published operation table (`INT-NET-057/1`).
const OPERATIONS: [(OperationType, u32); 4] = [
    (OperationType::Registration, 1),
    (OperationType::Topology, 2),
    (OperationType::CatalogPage, 3),
    (OperationType::Upload, 4),
];

/// A valid request of the given operation with the contract credentials.
fn request(operation: OperationType) -> ClientRequest {
    ClientRequest {
        operation,
        login: LOGIN.to_owned(),
        password: PASSWORD.to_owned(),
        args: vec![],
    }
}

/// A valid request with the contract operation and the given credentials.
fn request_with(login: &str, password: &str) -> ClientRequest {
    ClientRequest {
        login: login.to_owned(),
        password: password.to_owned(),
        ..request(OperationType::Topology)
    }
}

/// Builds a client-header array from its four positions.
fn header(operation: Value, login: Value, password: Value, args: Value) -> Value {
    Value::Array(vec![operation, login, password, args])
}

/// The valid contract header before any golden substitution.
fn valid_header() -> Value {
    header(
        Value::Uint(u64::from(OperationType::Topology.code())),
        Value::Text(LOGIN.to_owned()),
        Value::Text(PASSWORD.to_owned()),
        Value::Array(vec![]),
    )
}

/// Replaces one position of a client header with `replacement`.
fn with_position(value: Value, position: usize, replacement: Value) -> Value {
    let Value::Array(mut items) = value else {
        panic!("client header is an array");
    };
    items[position] = replacement;
    Value::Array(items)
}

/// Decodes a client header value with the published v1 limits.
fn decode_header(value: &Value) -> Result<ClientRequest, ClientError> {
    let Value::Array(items) = value else {
        panic!("client header is an array");
    };
    ClientRequest::from_args(items, LIMITS)
}

/// Encodes a request and decodes it back through the CBOR layer.
fn round_trip(request: &ClientRequest) -> ClientRequest {
    let args = request
        .to_args(LIMITS)
        .expect("request credentials are valid");
    let value = Value::Array(args);
    let decoded = cbor::decode(&cbor::encode(&value)).expect("header bytes are valid CBOR");
    decode_header(&decoded).expect("encoded header is valid")
}

/// `INT-NET-051/3`, `INT-NET-055/2`, `INT-NET-060/1`: the topology request is
/// `[2, login, password, []]`.
#[test]
fn topology_request_matches_contract_vector() {
    let request = request(OperationType::Topology);
    let args = request.to_args(LIMITS).expect("request is valid");
    let expected = {
        let mut bytes = vec![0x84, 0x02, 0x65];
        bytes.extend_from_slice(LOGIN.as_bytes());
        bytes.push(0x6B);
        bytes.extend_from_slice(PASSWORD.as_bytes());
        bytes.push(0x80);
        bytes
    };

    assert_eq!(expected.len(), 21);
    assert_eq!(cbor::encode(&Value::Array(args)), expected);
    assert_eq!(round_trip(&request), request);
}

/// `INT-NET-051/3`, `INT-NET-080/1`, `TST-028/1`: a catalog page request keeps
/// the `[snapshot_token, cursor]` arguments in the fourth position.
#[test]
fn catalog_request_matches_contract_vector() {
    let request = ClientRequest {
        args: CatalogPageRequest {
            snapshot_token: None,
            cursor: None,
        }
        .to_args()
        .expect("request arguments are valid"),
        ..request(OperationType::CatalogPage)
    };
    let args = request.to_args(LIMITS).expect("request is valid");
    let expected = {
        let mut bytes = vec![0x84, 0x03, 0x65];
        bytes.extend_from_slice(LOGIN.as_bytes());
        bytes.push(0x6B);
        bytes.extend_from_slice(PASSWORD.as_bytes());
        bytes.extend_from_slice(&[0x82, 0xF6, 0xF6]);
        bytes
    };

    assert_eq!(cbor::encode(&Value::Array(args)), expected);
    assert_eq!(round_trip(&request), request);
}

/// `INT-NET-057/1`: codes `1..=4` are assigned and `0`, `5`, and larger values
/// are unknown.
#[test]
fn operation_codes_match_the_contract_table() {
    for (operation, code) in OPERATIONS {
        assert_eq!(operation.code(), code);
        assert_eq!(OperationType::from_code(code), Ok(operation));
    }

    for code in [0, 5, u32::MAX] {
        assert_eq!(
            OperationType::from_code(code),
            Err(ClientError::UnknownOperation(u64::from(code)))
        );
    }
}

/// `CFG-CL-029`: the login length range is inclusive and counts ASCII bytes.
#[test]
fn login_accepts_exactly_the_configured_limits() {
    let shortest = request_with("a", PASSWORD);
    assert_eq!(round_trip(&shortest), shortest);

    let longest_login = "a".repeat(64);
    let longest = request_with(&longest_login, PASSWORD);
    assert_eq!(round_trip(&longest), longest);

    assert_eq!(
        request_with("", PASSWORD).to_args(LIMITS),
        Err(ClientError::LoginLength {
            actual: 0,
            min: 1,
            max: 64,
        })
    );
    assert_eq!(
        request_with(&"a".repeat(65), PASSWORD).to_args(LIMITS),
        Err(ClientError::LoginLength {
            actual: 65,
            min: 1,
            max: 64,
        })
    );

    let custom = CredentialLimits {
        min_login_bytes: 4,
        max_login_bytes: 8,
        ..LIMITS
    };
    let accepted = request_with("abcd", PASSWORD);
    let args = accepted.to_args(custom).expect("login is valid");
    assert_eq!(ClientRequest::from_args(&args, custom), Ok(accepted));
    assert_eq!(
        request_with("abc", PASSWORD).to_args(custom),
        Err(ClientError::LoginLength {
            actual: 3,
            min: 4,
            max: 8,
        })
    );
    assert_eq!(
        request_with("abcdefghi", PASSWORD).to_args(custom),
        Err(ClientError::LoginLength {
            actual: 9,
            min: 4,
            max: 8,
        })
    );
}

/// `INT-NET-054/1`: the login is restricted to `[A-Za-z0-9._-]`.
#[test]
fn login_rejects_characters_outside_the_allowed_set() {
    for (login, character) in [
        ("ali ce", ' '),
        ("алиса", 'а'),
        ("alice!", '!'),
        ("alice\n", '\n'),
    ] {
        assert_eq!(
            request_with(login, PASSWORD).to_args(LIMITS),
            Err(ClientError::InvalidLoginCharacter { character })
        );
    }
}

/// `INT-NET-054/1`: every published login character is accepted.
#[test]
fn login_allows_every_published_character() {
    let allowed = request_with("AZaz09._-", PASSWORD);

    assert_eq!(allowed.login.len(), 10);
    assert_eq!(round_trip(&allowed), allowed);
}

/// `CFG-CL-029`: the password range is inclusive and counts UTF-8 bytes.
#[test]
fn password_accepts_exactly_the_configured_limits() {
    let shortest = request_with(LOGIN, &"a".repeat(8));
    assert_eq!(round_trip(&shortest), shortest);

    let longest = request_with(LOGIN, &"a".repeat(1024));
    assert_eq!(round_trip(&longest), longest);

    assert_eq!(
        request_with(LOGIN, &"a".repeat(7)).to_args(LIMITS),
        Err(ClientError::PasswordLength {
            actual: 7,
            min: 8,
            max: 1024,
        })
    );
    assert_eq!(
        request_with(LOGIN, &"a".repeat(1025)).to_args(LIMITS),
        Err(ClientError::PasswordLength {
            actual: 1025,
            min: 8,
            max: 1024,
        })
    );

    let two_byte_password = "é".repeat(512);
    assert_eq!(two_byte_password.len(), 1024);
    let accepted = request_with(LOGIN, &two_byte_password);
    assert_eq!(round_trip(&accepted), accepted);

    assert_eq!(
        request_with(LOGIN, &"é".repeat(513)).to_args(LIMITS),
        Err(ClientError::PasswordLength {
            actual: 1026,
            min: 8,
            max: 1024,
        })
    );
}

/// `INT-NET-054/1`: the password is not Unicode-normalized.
#[test]
fn password_round_trips_unicode_without_normalization() {
    let composed = request_with(LOGIN, "éééé");
    let decomposed = request_with(LOGIN, "e\u{0301}e\u{0301}e\u{0301}e\u{0301}");

    let composed_bytes = cbor::encode(&Value::Array(
        composed.to_args(LIMITS).expect("password is valid"),
    ));
    let decomposed_bytes = cbor::encode(&Value::Array(
        decomposed.to_args(LIMITS).expect("password is valid"),
    ));

    assert_ne!(composed_bytes, decomposed_bytes);
    assert_eq!(round_trip(&composed).password, "éééé");
    assert_eq!(
        round_trip(&decomposed).password,
        "e\u{0301}e\u{0301}e\u{0301}e\u{0301}"
    );
}

/// `INT-NET-051/3`: the header is exactly four positions with their published
/// CBOR types.
#[test]
fn client_header_rejects_wrong_shapes_and_types() {
    let Value::Array(items) = valid_header() else {
        panic!("client header is an array");
    };

    assert_eq!(
        ClientRequest::from_args(&items[..3], LIMITS),
        Err(ClientError::WrongShape)
    );
    let mut longer = items.clone();
    longer.push(Value::Null);
    assert_eq!(
        ClientRequest::from_args(&longer, LIMITS),
        Err(ClientError::WrongShape)
    );
    assert_eq!(
        ClientRequest::from_args(&[Value::Null], LIMITS),
        Err(ClientError::WrongShape)
    );

    assert_eq!(
        decode_header(&with_position(
            valid_header(),
            0,
            Value::Text("2".to_owned())
        )),
        Err(ClientError::WrongType("operation_type"))
    );
    assert_eq!(
        decode_header(&with_position(valid_header(), 1, Value::Uint(1))),
        Err(ClientError::WrongType("login"))
    );
    assert_eq!(
        decode_header(&with_position(valid_header(), 2, Value::Uint(1))),
        Err(ClientError::WrongType("password"))
    );
    assert_eq!(
        decode_header(&with_position(valid_header(), 3, Value::Null)),
        Err(ClientError::WrongType("args"))
    );
}

/// `INT-NET-057/1`: operation codes outside `1..=4` are rejected, including
/// values above `uint32`.
#[test]
fn client_header_rejects_unknown_operation_codes() {
    for code in [0, 5, u64::from(u32::MAX) + 1] {
        assert_eq!(
            decode_header(&with_position(valid_header(), 0, Value::Uint(code))),
            Err(ClientError::UnknownOperation(code))
        );
    }
}

/// `INT-NET-054/1`: invalid credentials cannot be encoded.
#[test]
fn encode_rejects_invalid_credentials() {
    assert_eq!(
        request_with(&"a".repeat(65), PASSWORD).to_args(LIMITS),
        Err(ClientError::LoginLength {
            actual: 65,
            min: 1,
            max: 64,
        })
    );
    assert_eq!(
        request_with(LOGIN, &"a".repeat(7)).to_args(LIMITS),
        Err(ClientError::PasswordLength {
            actual: 7,
            min: 8,
            max: 1024,
        })
    );
}
