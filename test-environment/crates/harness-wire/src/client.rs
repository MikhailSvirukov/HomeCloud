//! Authenticated client logical header.
//!
//! A client request is the four-position CBOR array
//! `[operation_type, login, password, args]` (`INT-NET-051 / 3`). The
//! operation-argument schema is determined by the operation type and validated
//! by the caller; this module validates only that the fourth position is a CBOR
//! array.

use crate::cbor::Value;

const OPERATION_REGISTRATION: u32 = 1;
const OPERATION_TOPOLOGY: u32 = 2;
const OPERATION_CATALOG_PAGE: u32 = 3;
const OPERATION_UPLOAD: u32 = 4;

const OPERATION_TYPE_FIELD: &str = "operation_type";
const LOGIN_FIELD: &str = "login";
const PASSWORD_FIELD: &str = "password";
const ARGS_FIELD: &str = "args";

/// A v1 client operation code (`INT-NET-057 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// Account and installation registration, code `1`.
    Registration,
    /// Topology retrieval, code `2`.
    Topology,
    /// Catalog-page request, code `3`.
    CatalogPage,
    /// Media upload, code `4`.
    Upload,
}

impl OperationType {
    /// The published `uint32` code.
    pub const fn code(self) -> u32 {
        match self {
            Self::Registration => OPERATION_REGISTRATION,
            Self::Topology => OPERATION_TOPOLOGY,
            Self::CatalogPage => OPERATION_CATALOG_PAGE,
            Self::Upload => OPERATION_UPLOAD,
        }
    }

    /// Resolves a published code; `0` and unassigned codes are rejected.
    pub fn from_code(code: u32) -> Result<Self, ClientError> {
        match code {
            OPERATION_REGISTRATION => Ok(Self::Registration),
            OPERATION_TOPOLOGY => Ok(Self::Topology),
            OPERATION_CATALOG_PAGE => Ok(Self::CatalogPage),
            OPERATION_UPLOAD => Ok(Self::Upload),
            _ => Err(ClientError::UnknownOperation(u64::from(code))),
        }
    }
}

/// Configured credential limits (`CFG-CL-029 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialLimits {
    /// Minimum login length in ASCII bytes.
    pub min_login_bytes: usize,
    /// Maximum login length in ASCII bytes.
    pub max_login_bytes: usize,
    /// Minimum password length in UTF-8 bytes.
    pub min_password_bytes: usize,
    /// Maximum password length in UTF-8 bytes.
    pub max_password_bytes: usize,
}

impl CredentialLimits {
    /// Published v1 defaults: login 1..=64 ASCII bytes, password 8..=1024 UTF-8 bytes.
    pub const V1_DEFAULT: Self = Self {
        min_login_bytes: 1,
        max_login_bytes: 64,
        min_password_bytes: 8,
        max_password_bytes: 1024,
    };
}

/// The authenticated client logical header `[operation_type, login, password, args]`
/// (`INT-NET-051 / 3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRequest {
    /// The operation this request performs.
    pub operation: OperationType,
    /// Login restricted to the v1 ASCII set.
    pub login: String,
    /// Password used unchanged as UTF-8 bytes.
    pub password: String,
    /// Operation arguments whose schema is determined by `operation`.
    pub args: Vec<Value>,
}

impl ClientRequest {
    /// `[operation_type, login, password, args]`; validates the credential limits.
    pub fn to_args(&self, limits: CredentialLimits) -> Result<Vec<Value>, ClientError> {
        validate_credentials(&self.login, &self.password, limits)?;

        Ok(vec![
            Value::Uint(u64::from(self.operation.code())),
            Value::Text(self.login.clone()),
            Value::Text(self.password.clone()),
            Value::Array(self.args.clone()),
        ])
    }

    /// Decodes and validates the four logical-header positions.
    pub fn from_args(args: &[Value], limits: CredentialLimits) -> Result<Self, ClientError> {
        let [operation, login, password, operation_args] = args else {
            return Err(ClientError::WrongShape);
        };

        let Value::Uint(code) = operation else {
            return Err(ClientError::WrongType(OPERATION_TYPE_FIELD));
        };
        let code = u32::try_from(*code).map_err(|_| ClientError::UnknownOperation(*code))?;
        let operation = OperationType::from_code(code)?;

        let Value::Text(login) = login else {
            return Err(ClientError::WrongType(LOGIN_FIELD));
        };
        let Value::Text(password) = password else {
            return Err(ClientError::WrongType(PASSWORD_FIELD));
        };
        let Value::Array(operation_args) = operation_args else {
            return Err(ClientError::WrongType(ARGS_FIELD));
        };

        validate_credentials(login, password, limits)?;

        Ok(Self {
            operation,
            login: login.clone(),
            password: password.clone(),
            args: operation_args.clone(),
        })
    }
}

/// Reasons a client logical header is not valid for protocol version 1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The value is not a four-element CBOR array.
    #[error("client logical header must be a four-element array")]
    WrongShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// The operation code is not assigned in the v1 operation table.
    #[error("unknown operation code {0}")]
    UnknownOperation(u64),
    /// The login contains a character outside the v1 ASCII set.
    #[error("login character {character:?} is not allowed")]
    InvalidLoginCharacter { character: char },
    /// The login length is outside the configured range.
    #[error("login is {actual} bytes, expected {min}..={max}")]
    LoginLength {
        actual: usize,
        min: usize,
        max: usize,
    },
    /// The password length is outside the configured range.
    #[error("password is {actual} bytes, expected {min}..={max}")]
    PasswordLength {
        actual: usize,
        min: usize,
        max: usize,
    },
}

fn validate_credentials(
    login: &str,
    password: &str,
    limits: CredentialLimits,
) -> Result<(), ClientError> {
    validate_login(login, limits)?;
    validate_password(password, limits)
}

fn validate_login(login: &str, limits: CredentialLimits) -> Result<(), ClientError> {
    if let Some(character) = login
        .chars()
        .find(|character| !is_login_character(*character))
    {
        return Err(ClientError::InvalidLoginCharacter { character });
    }

    let actual = login.len();
    if actual < limits.min_login_bytes || actual > limits.max_login_bytes {
        Err(ClientError::LoginLength {
            actual,
            min: limits.min_login_bytes,
            max: limits.max_login_bytes,
        })
    } else {
        Ok(())
    }
}

fn validate_password(password: &str, limits: CredentialLimits) -> Result<(), ClientError> {
    let actual = password.len();
    if actual < limits.min_password_bytes || actual > limits.max_password_bytes {
        Err(ClientError::PasswordLength {
            actual,
            min: limits.min_password_bytes,
            max: limits.max_password_bytes,
        })
    } else {
        Ok(())
    }
}

fn is_login_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}
