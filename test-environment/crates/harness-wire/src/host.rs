//! Canonical IP literal validation for the topology `host` position.
//!
//! A v1 topology host is an unbracketed canonical IPv4 or IPv6 literal
//! (`INT-NET-070 / 6`). DNS names, non-canonical IPv6 spellings, and IPv6 zone
//! identifiers are rejected.

use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

/// A canonical IPv4 or IPv6 literal accepted in the topology `host` position
/// (`INT-NET-070 / 6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalHost(String);

/// Reasons a topology host is not a canonical IP literal.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The host text is empty.
    #[error("host is empty")]
    Empty,
    /// The host text contains an IPv6 zone identifier.
    #[error("host contains an IPv6 zone identifier")]
    ZoneIdentifier,
    /// The host text is not an IP literal.
    #[error("host is not an IP literal")]
    NotAnIpLiteral,
    /// The host text looks like a DNS name.
    #[error("host is a DNS name, not an IP literal")]
    DnsName,
    /// The host parses as IPv6 but is not in RFC 5952 canonical form.
    #[error("IPv6 host is not in RFC 5952 canonical form")]
    NonCanonicalIpv6,
}

impl CanonicalHost {
    /// Parses and validates a canonical IPv4 or IPv6 literal.
    pub fn parse(value: &str) -> Result<Self, HostError> {
        if value.is_empty() {
            return Err(HostError::Empty);
        }

        if value.contains('%') {
            return Err(HostError::ZoneIdentifier);
        }

        if let Ok(address) = Ipv4Addr::from_str(value)
            && address.to_string() == value
        {
            return Ok(Self(value.to_owned()));
        }

        if let Ok(address) = Ipv6Addr::from_str(value) {
            if address.to_string() == value {
                return Ok(Self(value.to_owned()));
            }
            return Err(HostError::NonCanonicalIpv6);
        }

        if !value.contains(':') && value.bytes().any(|byte| byte.is_ascii_alphabetic()) {
            Err(HostError::DnsName)
        } else {
            Err(HostError::NotAnIpLiteral)
        }
    }

    /// The exact canonical text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CanonicalHost {
    type Err = HostError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for CanonicalHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
