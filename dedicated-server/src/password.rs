//! Argon2id password verifiers stored as PHC records.
//!
//! Deriving a verifier uses Argon2id version `0x13` with a fresh salt of the
//! configured length from the operating system CSPRNG. Checking a password reads
//! the salt, parameters and algorithm version back from the stored record, so a
//! later configuration change cannot invalidate existing verifiers. This module
//! performs no I/O.

use std::num::{NonZeroU32, NonZeroUsize};

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier as _, SaltString};
use argon2::{Algorithm, Argon2, Params, Version, MIN_SALT_LEN};
use thiserror::Error;

/// Maximum raw salt length that a PHC record can carry, in bytes.
///
/// A PHC salt is at most 64 base64 characters, which unpadded encoding maps to
/// 48 raw bytes.
const MAX_SALT_BYTES: usize = 48;

/// PHC parameter names for memory cost, iteration count and parallelism.
const MEMORY_PARAM: &str = "m";
const ITERATIONS_PARAM: &str = "t";
const PARALLELISM_PARAM: &str = "p";

/// Result of deriving, parsing or checking a [`PasswordVerifier`].
type Result<T, E = PasswordError> = core::result::Result<T, E>;

/// Argon2id parameters used when deriving a new password verifier.
#[derive(Clone, Copy)]
pub struct Argon2Params {
    /// Memory cost in kibibytes.
    pub memory_kib: NonZeroU32,
    /// Number of passes.
    pub iterations: NonZeroU32,
    /// Degree of parallelism.
    pub parallelism: NonZeroU32,
    /// Salt length in bytes.
    pub salt_length_bytes: NonZeroUsize,
}

/// A password verifier stored as a PHC record.
pub struct PasswordVerifier {
    phc: String,
}

impl PasswordVerifier {
    /// Derive a verifier for `password` with a fresh random salt and `params`.
    pub fn derive(password: &str, params: Argon2Params) -> Result<Self> {
        let Argon2Params {
            memory_kib,
            iterations,
            parallelism,
            salt_length_bytes,
        } = params;
        let params = Params::new(memory_kib.get(), iterations.get(), parallelism.get(), None)
            .map_err(PasswordError::Parameters)?;
        let salt_length = salt_length_bytes.get();
        if !(MIN_SALT_LEN..=MAX_SALT_BYTES).contains(&salt_length) {
            return Err(PasswordError::SaltLength(salt_length));
        }
        let mut salt_bytes = vec![0_u8; salt_length];
        OsRng.fill_bytes(&mut salt_bytes);
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|_| PasswordError::SaltLength(salt_length))?;
        let hash = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password.as_bytes(), &salt)
            .map_err(PasswordError::Hash)?;
        Ok(Self {
            phc: hash.to_string(),
        })
    }

    /// Adopt a stored PHC record.
    pub fn parse(phc: &str) -> Result<Self> {
        let hash = PasswordHash::new(phc).map_err(PasswordError::MalformedRecord)?;
        validated_params(&hash)?;
        Ok(Self {
            phc: phc.to_owned(),
        })
    }

    /// Check `password` against this record using its stored salt and parameters.
    pub fn verify(&self, password: &str) -> Result<bool> {
        let hash = PasswordHash::new(&self.phc).map_err(PasswordError::MalformedRecord)?;
        let params = validated_params(&hash)?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        match argon2.verify_password(password.as_bytes(), &hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(source) => Err(PasswordError::Verify(source)),
        }
    }

    /// The PHC record as text.
    pub fn as_str(&self) -> &str {
        &self.phc
    }
}

/// Validate a record for Argon2id version `0x13` and return its stored parameters.
fn validated_params(hash: &PasswordHash<'_>) -> Result<Params> {
    match Algorithm::try_from(hash.algorithm) {
        Ok(Algorithm::Argon2id) => {}
        _ => return Err(PasswordError::UnsupportedAlgorithm),
    }
    match hash.version {
        Some(version) if version == u32::from(Version::V0x13) => {}
        _ => return Err(PasswordError::UnsupportedVersion),
    }
    if hash.hash.is_none() {
        return Err(malformed_record());
    }
    let Some(salt) = hash.salt.as_ref() else {
        return Err(malformed_record());
    };
    let mut salt_bytes = [0_u8; MAX_SALT_BYTES];
    let salt_bytes = salt
        .decode_b64(&mut salt_bytes)
        .map_err(PasswordError::MalformedRecord)?;
    if !(MIN_SALT_LEN..=MAX_SALT_BYTES).contains(&salt_bytes.len()) {
        return Err(malformed_record());
    }
    for name in [MEMORY_PARAM, ITERATIONS_PARAM, PARALLELISM_PARAM] {
        if hash.params.get(name).is_none() {
            return Err(malformed_record());
        }
    }
    Params::try_from(hash).map_err(PasswordError::MalformedRecord)
}

/// The record is structurally valid PHC text but is not a usable verifier.
fn malformed_record() -> PasswordError {
    PasswordError::MalformedRecord(argon2::password_hash::Error::PhcStringField)
}

/// Errors produced while deriving or checking a verifier.
#[derive(Debug, Error)]
pub enum PasswordError {
    /// The algorithm rejected the requested parameters.
    #[error("invalid Argon2 parameters: {0}")]
    Parameters(#[source] argon2::Error),
    /// The configured salt length is outside Argon2's accepted range.
    #[error(
        "salt length of {0} bytes is outside the accepted range of {min} to {max} bytes",
        min = MIN_SALT_LEN,
        max = MAX_SALT_BYTES
    )]
    SaltLength(usize),
    /// Hashing the password failed inside the hashing API.
    ///
    /// Derivation validates the parameters and salt first, so this is a
    /// defensive mapping of the fallible hashing API rather than an expected
    /// failure.
    #[error("failed to derive password verifier: {0}")]
    Hash(#[source] argon2::password_hash::Error),
    /// The text is not a usable PHC password record.
    #[error("invalid PHC password record: {0}")]
    MalformedRecord(#[source] argon2::password_hash::Error),
    /// The record does not use the Argon2id algorithm.
    #[error("password verifier must use the argon2id algorithm")]
    UnsupportedAlgorithm,
    /// The record does not use Argon2 version 0x13.
    #[error("password verifier must use Argon2 version 0x13")]
    UnsupportedVersion,
    /// Checking the password against the record failed.
    #[error("failed to verify password: {0}")]
    Verify(#[source] argon2::password_hash::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct horse battery staple";
    const WRONG_PASSWORD: &str = "correct horse battery stapler";
    const MEMORY_KIB: u32 = 8;
    const ITERATIONS: u32 = 1;
    const PARALLELISM: u32 = 1;
    const SALT_LENGTH_BYTES: usize = 16;

    // Argon2id v=19 with m=16, t=2, p=1 and salt "b3RoZXJwYXJhbXM" ("otherparams").
    const PINNED_OTHER_PARAMS_RECORD: &str =
        "$argon2id$v=19$m=16,t=2,p=1$b3RoZXJwYXJhbXM$YIFp3JbhwEcWxcnpIMBmeRUVWpA2AmBrJLTHa4XAFcA";
    const PINNED_OTHER_PARAMS_PASSWORD: &str = "other parameters password";
    const OTHER_MEMORY_KIB: u32 = 16;
    const OTHER_ITERATIONS: u32 = 2;
    const OTHER_PARALLELISM: u32 = 1;

    // Local regression pin only: the shared INT-NET-046 vector is not published
    // yet, so this is not the cross-component vector. Produced with argon2 0.5.3
    // (Argon2id, version 0x13, m=8 KiB, t=1, p=1), 11-byte salt "knownanswer"
    // encoded as "a25vd25hbnN3ZXI", and password "known answer password";
    // replace it with the published cross-component vector once one exists.
    const KNOWN_ANSWER_RECORD: &str =
        "$argon2id$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const KNOWN_ANSWER_PASSWORD: &str = "known answer password";
    const KNOWN_ANSWER_WRONG_PASSWORD: &str = "known answer passphrase";

    const OTHER_ALGORITHM_RECORD: &str =
        "$argon2i$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const OTHER_VERSION_RECORD: &str =
        "$argon2id$v=16$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const MISSING_PARAMETER_RECORD: &str =
        "$argon2id$v=19$m=8,t=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const MISSING_HASH_RECORD: &str = "$argon2id$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI";
    const SHORT_SALT_RECORD: &str =
        "$argon2id$v=19$m=8,t=1,p=1$a25v$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const BELOW_MINIMUM_MEMORY_RECORD: &str =
        "$argon2id$v=19$m=4,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";

    fn non_zero(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test values must be non-zero")
    }

    fn non_zero_usize(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test values must be non-zero")
    }

    fn params(memory_kib: u32, iterations: u32, parallelism: u32) -> Argon2Params {
        Argon2Params {
            memory_kib: non_zero(memory_kib),
            iterations: non_zero(iterations),
            parallelism: non_zero(parallelism),
            salt_length_bytes: non_zero_usize(SALT_LENGTH_BYTES),
        }
    }

    fn params_with_salt(salt_length_bytes: usize) -> Argon2Params {
        Argon2Params {
            salt_length_bytes: non_zero_usize(salt_length_bytes),
            ..fast_params()
        }
    }

    fn fast_params() -> Argon2Params {
        params(MEMORY_KIB, ITERATIONS, PARALLELISM)
    }

    fn record(phc: &str) -> PasswordVerifier {
        PasswordVerifier::parse(phc).expect("test record must parse")
    }

    /// Bypass adoption to exercise verification's own validation of stored text.
    fn unvalidated(phc: &str) -> PasswordVerifier {
        PasswordVerifier {
            phc: phc.to_owned(),
        }
    }

    fn decimal_parameter(hash: &PasswordHash<'_>, name: &str) -> u32 {
        hash.params
            .get(name)
            .and_then(|value| value.decimal().ok())
            .expect("test parameter must be a decimal")
    }

    #[test]
    fn derive_records_argon2id_version_19_and_the_requested_parameters() {
        let verifier =
            PasswordVerifier::derive(PASSWORD, fast_params()).expect("derivation must succeed");

        let hash = PasswordHash::new(verifier.as_str()).expect("derived record must parse");

        assert_eq!(hash.algorithm.as_str(), "argon2id");
        assert_eq!(hash.version, Some(19));
        assert_eq!(decimal_parameter(&hash, MEMORY_PARAM), MEMORY_KIB);
        assert_eq!(decimal_parameter(&hash, ITERATIONS_PARAM), ITERATIONS);
        assert_eq!(decimal_parameter(&hash, PARALLELISM_PARAM), PARALLELISM);
    }

    #[test]
    fn derive_generates_the_configured_salt_length() {
        let configured = 24;
        let verifier = PasswordVerifier::derive(PASSWORD, params_with_salt(configured))
            .expect("derivation must succeed");
        let hash = PasswordHash::new(verifier.as_str()).expect("derived record must parse");
        let salt = hash.salt.expect("derived record must carry a salt");
        let mut decoded = [0_u8; 64];

        let decoded = salt
            .decode_b64(&mut decoded)
            .expect("derived salt must decode");

        assert_eq!(decoded.len(), configured);
    }

    #[test]
    fn verify_accepts_the_derived_password() {
        let verifier =
            PasswordVerifier::derive(PASSWORD, fast_params()).expect("derivation must succeed");

        assert!(verifier
            .verify(PASSWORD)
            .expect("verification must succeed"));
    }

    #[test]
    fn verify_rejects_a_different_password() {
        let verifier =
            PasswordVerifier::derive(PASSWORD, fast_params()).expect("derivation must succeed");

        assert!(!verifier
            .verify(WRONG_PASSWORD)
            .expect("verification must succeed"));
    }

    #[test]
    fn separate_derivations_use_distinct_salts_and_both_verify() {
        let first = PasswordVerifier::derive(PASSWORD, fast_params())
            .expect("first derivation must succeed");
        let second = PasswordVerifier::derive(PASSWORD, fast_params())
            .expect("second derivation must succeed");

        let first_hash = PasswordHash::new(first.as_str()).expect("first record must parse");
        let second_hash = PasswordHash::new(second.as_str()).expect("second record must parse");

        assert_ne!(first.as_str(), second.as_str());
        assert_ne!(first_hash.salt, second_hash.salt);
        assert!(first.verify(PASSWORD).expect("first check must succeed"));
        assert!(second.verify(PASSWORD).expect("second check must succeed"));
    }

    #[test]
    fn verify_uses_the_parameters_stored_in_the_record() {
        let verifier = record(PINNED_OTHER_PARAMS_RECORD);
        let hash = PasswordHash::new(verifier.as_str()).expect("pinned record must parse");

        assert_eq!(decimal_parameter(&hash, MEMORY_PARAM), OTHER_MEMORY_KIB);
        assert_eq!(decimal_parameter(&hash, ITERATIONS_PARAM), OTHER_ITERATIONS);
        assert_eq!(
            decimal_parameter(&hash, PARALLELISM_PARAM),
            OTHER_PARALLELISM
        );
        assert!(verifier
            .verify(PINNED_OTHER_PARAMS_PASSWORD)
            .expect("verification must succeed"));
    }

    #[test]
    fn known_answer_vector_accepts_only_its_password() {
        let verifier = record(KNOWN_ANSWER_RECORD);

        assert!(verifier
            .verify(KNOWN_ANSWER_PASSWORD)
            .expect("verification must succeed"));
        assert!(!verifier
            .verify(KNOWN_ANSWER_WRONG_PASSWORD)
            .expect("verification must succeed"));
    }

    #[test]
    fn parse_and_verify_reject_a_malformed_record() {
        assert!(matches!(
            PasswordVerifier::parse("not a PHC record"),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            unvalidated("not a PHC record").verify(PASSWORD),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            PasswordVerifier::parse(MISSING_HASH_RECORD),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            unvalidated(MISSING_HASH_RECORD).verify(PASSWORD),
            Err(PasswordError::MalformedRecord(_))
        ));
    }

    #[test]
    fn parse_and_verify_reject_a_non_argon2id_record() {
        assert!(matches!(
            PasswordVerifier::parse(OTHER_ALGORITHM_RECORD),
            Err(PasswordError::UnsupportedAlgorithm)
        ));
        assert!(matches!(
            unvalidated(OTHER_ALGORITHM_RECORD).verify(PASSWORD),
            Err(PasswordError::UnsupportedAlgorithm)
        ));
    }

    #[test]
    fn parse_and_verify_reject_a_non_v0x13_record() {
        assert!(matches!(
            PasswordVerifier::parse(OTHER_VERSION_RECORD),
            Err(PasswordError::UnsupportedVersion)
        ));
        assert!(matches!(
            unvalidated(OTHER_VERSION_RECORD).verify(PASSWORD),
            Err(PasswordError::UnsupportedVersion)
        ));
    }

    #[test]
    fn parse_and_verify_reject_a_record_missing_a_parameter() {
        assert!(matches!(
            PasswordVerifier::parse(MISSING_PARAMETER_RECORD),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            unvalidated(MISSING_PARAMETER_RECORD).verify(PASSWORD),
            Err(PasswordError::MalformedRecord(_))
        ));
    }

    #[test]
    fn parse_and_verify_reject_a_record_with_a_short_salt() {
        assert!(matches!(
            PasswordVerifier::parse(SHORT_SALT_RECORD),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            unvalidated(SHORT_SALT_RECORD).verify(PASSWORD),
            Err(PasswordError::MalformedRecord(_))
        ));
    }

    #[test]
    fn verify_rejects_a_record_with_memory_below_the_minimum() {
        assert!(matches!(
            unvalidated(BELOW_MINIMUM_MEMORY_RECORD).verify(PASSWORD),
            Err(PasswordError::MalformedRecord(_))
        ));
    }

    #[test]
    fn derive_rejects_parameters_outside_the_algorithm_limits() {
        assert!(matches!(
            PasswordVerifier::derive(PASSWORD, params(1, 1, 1)),
            Err(PasswordError::Parameters(_))
        ));
    }

    #[test]
    fn derive_rejects_an_unusable_salt_length() {
        for length in [7, 49] {
            assert!(matches!(
                PasswordVerifier::derive(PASSWORD, params_with_salt(length)),
                Err(PasswordError::SaltLength(rejected)) if rejected == length
            ));
        }
    }
}
