//! Argon2id password verifiers stored as PHC records.
//!
//! Deriving a verifier uses Argon2id version `0x13` with a fresh salt from the
//! operating system CSPRNG. Checking a password reads the salt, parameters and
//! algorithm version back from the stored record, so a later configuration
//! change cannot invalidate existing verifiers. This module performs no I/O.

use std::num::NonZeroU32;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier as _, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use thiserror::Error;

/// Result of deriving, parsing or checking a [`PasswordVerifier`].
type Result<T, E = PasswordError> = core::result::Result<T, E>;

/// Argon2id parameters used when deriving a new password verifier.
#[derive(Debug, Clone, Copy)]
pub struct Argon2Params {
    /// Memory cost in kibibytes.
    pub memory_kib: NonZeroU32,
    /// Number of passes.
    pub iterations: NonZeroU32,
    /// Degree of parallelism.
    pub parallelism: NonZeroU32,
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
        } = params;
        let params = Params::new(memory_kib.get(), iterations.get(), parallelism.get(), None)
            .map_err(PasswordError::Parameters)?;
        let salt = SaltString::generate(&mut OsRng);
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
        if hash.salt.is_none() || hash.hash.is_none() {
            return Err(PasswordError::MalformedRecord(
                argon2::password_hash::Error::PhcStringField,
            ));
        }
        Ok(Self {
            phc: phc.to_owned(),
        })
    }

    /// Check `password` against this record using its stored salt and parameters.
    pub fn verify(&self, password: &str) -> Result<bool> {
        let hash = PasswordHash::new(&self.phc).map_err(PasswordError::MalformedRecord)?;
        let algorithm = match Algorithm::try_from(hash.algorithm) {
            Ok(Algorithm::Argon2id) => Algorithm::Argon2id,
            _ => return Err(PasswordError::UnsupportedAlgorithm),
        };
        let version = match hash.version {
            Some(version) if version == u32::from(Version::V0x13) => Version::V0x13,
            _ => return Err(PasswordError::UnsupportedVersion),
        };
        let params = Params::try_from(&hash).map_err(PasswordError::MalformedRecord)?;
        let argon2 = Argon2::new(algorithm, version, params);
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

/// Errors produced while deriving or checking a verifier.
#[derive(Debug, Error)]
pub enum PasswordError {
    /// The algorithm rejected the requested parameters.
    #[error("invalid Argon2 parameters: {0}")]
    Parameters(#[source] argon2::Error),
    /// Deriving the verifier failed.
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

    // Argon2id v=19 with m=16, t=2, p=1 and salt "b3RoZXJwYXJhbXM" ("otherparams").
    const PINNED_OTHER_PARAMS_RECORD: &str =
        "$argon2id$v=19$m=16,t=2,p=1$b3RoZXJwYXJhbXM$YIFp3JbhwEcWxcnpIMBmeRUVWpA2AmBrJLTHa4XAFcA";
    const PINNED_OTHER_PARAMS_PASSWORD: &str = "other parameters password";
    const OTHER_MEMORY_KIB: u32 = 16;
    const OTHER_ITERATIONS: u32 = 2;
    const OTHER_PARALLELISM: u32 = 1;

    // Argon2id v=19 with m=8, t=1, p=1 and salt "a25vd25hbnN3ZXI" ("knownanswer").
    const KNOWN_ANSWER_RECORD: &str =
        "$argon2id$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const KNOWN_ANSWER_PASSWORD: &str = "known answer password";
    const KNOWN_ANSWER_WRONG_PASSWORD: &str = "known answer passphrase";

    const OTHER_ALGORITHM_RECORD: &str =
        "$argon2i$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";
    const OTHER_VERSION_RECORD: &str =
        "$argon2id$v=16$m=8,t=1,p=1$a25vd25hbnN3ZXI$DP4mHqE7C+N9peSJezbHkEO9xfDPZGs3rs6E9ccF+Xk";

    fn non_zero(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test values must be non-zero")
    }

    fn params(memory_kib: u32, iterations: u32, parallelism: u32) -> Argon2Params {
        Argon2Params {
            memory_kib: non_zero(memory_kib),
            iterations: non_zero(iterations),
            parallelism: non_zero(parallelism),
        }
    }

    fn fast_params() -> Argon2Params {
        params(MEMORY_KIB, ITERATIONS, PARALLELISM)
    }

    fn record(phc: &str) -> PasswordVerifier {
        PasswordVerifier::parse(phc).expect("test record must parse")
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
        assert_eq!(decimal_parameter(&hash, "m"), MEMORY_KIB);
        assert_eq!(decimal_parameter(&hash, "t"), ITERATIONS);
        assert_eq!(decimal_parameter(&hash, "p"), PARALLELISM);
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

        assert_eq!(decimal_parameter(&hash, "m"), OTHER_MEMORY_KIB);
        assert_eq!(decimal_parameter(&hash, "t"), OTHER_ITERATIONS);
        assert_eq!(decimal_parameter(&hash, "p"), OTHER_PARALLELISM);
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
    fn invalid_records_and_parameters_are_rejected() {
        assert!(matches!(
            PasswordVerifier::parse("argon2id"),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            PasswordVerifier::parse("$argon2id$v=19$m=8,t=1,p=1$a25vd25hbnN3ZXI"),
            Err(PasswordError::MalformedRecord(_))
        ));
        assert!(matches!(
            record(OTHER_ALGORITHM_RECORD).verify(PASSWORD),
            Err(PasswordError::UnsupportedAlgorithm)
        ));
        assert!(matches!(
            record(OTHER_VERSION_RECORD).verify(PASSWORD),
            Err(PasswordError::UnsupportedVersion)
        ));
        assert!(matches!(
            PasswordVerifier::derive(PASSWORD, params(1, 1, 1)),
            Err(PasswordError::Parameters(_))
        ));
    }
}
