//! Published v1 result codes (`INT-NET-062 / 1`, `INT-NET-063 / 1`).
//!
//! The contract fixes one numeric value per meaning and reserves one 16-bit
//! range per class (contract tables 5 and 6). A response carries a `uint32`
//! result code; this module names the published values the server may send and
//! classifies any value by its reserved range, so a future request handler
//! cannot invent a code or assign a published value a different meaning.
//!
//! The envelope codec deliberately keeps decoding arbitrary values: a peer that
//! speaks a newer revision may send a code this build does not name yet.

/// Class of a result code, taken from its reserved range (contract table 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    /// `0x0001_0000-0x0001_FFFF`: request errors.
    Request,
    /// `0x0002_0000-0x0002_FFFF`: authentication errors.
    Authentication,
    /// `0x0003_0000-0x0003_FFFF`: registration errors.
    Registration,
    /// `0x0004_0000-0x0004_FFFF`: catalog errors.
    Catalog,
    /// `0x0005_0000-0x0005_FFFF`: file and upload errors.
    File,
    /// `0x0006_0000-0x0006_FFFF`: quorum and cluster errors.
    Quorum,
    /// `0x0007_0000-0x0007_FFFF`: resource errors.
    Resource,
    /// `0x0008_0000-0x0008_FFFF`: internal errors.
    Internal,
}

impl ResultClass {
    /// Class of any code inside a reserved range.
    ///
    /// Success (`0`) and every value outside the eight reserved ranges return
    /// `None`; such a value remains reserved and must not be sent.
    pub const fn from_u32(code: u32) -> Option<Self> {
        match code >> 16 {
            0x0001 => Some(Self::Request),
            0x0002 => Some(Self::Authentication),
            0x0003 => Some(Self::Registration),
            0x0004 => Some(Self::Catalog),
            0x0005 => Some(Self::File),
            0x0006 => Some(Self::Quorum),
            0x0007 => Some(Self::Resource),
            0x0008 => Some(Self::Internal),
            _ => None,
        }
    }
}

/// A published v1 result code (contract table 5).
///
/// The variants are failure codes only: success is the numeric value `0`
/// reported by [`crate::ResultEnvelope::success`] (`INT-NET-062 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCode {
    /// `0x0001_0001`: the operation code is not known.
    UnknownOperation,
    /// `0x0001_0002`: the arguments are invalid.
    InvalidArguments,
    /// `0x0001_0003`: the request is not valid in the current state.
    InvalidRequestState,
    /// `0x0001_0004`: the request identifier was reused with different content.
    RepeatedRequestId,
    /// `0x0002_0001`: authentication was rejected.
    AuthenticationRejected,
    /// `0x0003_0001`: registration conflicted with an existing account.
    RegistrationConflict,
    /// `0x0003_0002`: registration was rejected by policy.
    RegistrationRejected,
    /// `0x0004_0001`: the catalog cursor is invalid.
    InvalidCursor,
    /// `0x0004_0002`: the catalog snapshot expired.
    ExpiredSnapshot,
    /// `0x0005_0001`: the file exceeds the configured size limit.
    FileSizeExceeded,
    /// `0x0005_0002`: size, metadata or hash mismatch.
    SizeMismatch,
    /// `0x0005_0003`: the frame sequence is invalid.
    InvalidFrameSequence,
    /// `0x0005_0004`: the upload identifier is already used.
    UploadIdConflict,
    /// `0x0006_0001`: the Raft quorum is unavailable.
    QuorumUnavailable,
    /// `0x0006_0002`: the blob replication quorum was not reached.
    ReplicationQuorumNotReached,
    /// `0x0007_0001`: the service is temporarily unavailable.
    TemporaryUnavailable,
    /// `0x0007_0002`: the storage capacity is exhausted.
    StorageExhausted,
    /// `0x0008_0001`: an internal error occurred.
    Internal,
}

impl ResultCode {
    /// Numeric value sent on the wire (contract table 5).
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::UnknownOperation => 0x0001_0001,
            Self::InvalidArguments => 0x0001_0002,
            Self::InvalidRequestState => 0x0001_0003,
            Self::RepeatedRequestId => 0x0001_0004,
            Self::AuthenticationRejected => 0x0002_0001,
            Self::RegistrationConflict => 0x0003_0001,
            Self::RegistrationRejected => 0x0003_0002,
            Self::InvalidCursor => 0x0004_0001,
            Self::ExpiredSnapshot => 0x0004_0002,
            Self::FileSizeExceeded => 0x0005_0001,
            Self::SizeMismatch => 0x0005_0002,
            Self::InvalidFrameSequence => 0x0005_0003,
            Self::UploadIdConflict => 0x0005_0004,
            Self::QuorumUnavailable => 0x0006_0001,
            Self::ReplicationQuorumNotReached => 0x0006_0002,
            Self::TemporaryUnavailable => 0x0007_0001,
            Self::StorageExhausted => 0x0007_0002,
            Self::Internal => 0x0008_0001,
        }
    }

    /// Published code for a numeric value, if this build names it.
    ///
    /// Unused values inside a reserved range return `None`; they stay reserved
    /// and must not be sent (`INT-NET-063 / 1`).
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x0001_0001 => Some(Self::UnknownOperation),
            0x0001_0002 => Some(Self::InvalidArguments),
            0x0001_0003 => Some(Self::InvalidRequestState),
            0x0001_0004 => Some(Self::RepeatedRequestId),
            0x0002_0001 => Some(Self::AuthenticationRejected),
            0x0003_0001 => Some(Self::RegistrationConflict),
            0x0003_0002 => Some(Self::RegistrationRejected),
            0x0004_0001 => Some(Self::InvalidCursor),
            0x0004_0002 => Some(Self::ExpiredSnapshot),
            0x0005_0001 => Some(Self::FileSizeExceeded),
            0x0005_0002 => Some(Self::SizeMismatch),
            0x0005_0003 => Some(Self::InvalidFrameSequence),
            0x0005_0004 => Some(Self::UploadIdConflict),
            0x0006_0001 => Some(Self::QuorumUnavailable),
            0x0006_0002 => Some(Self::ReplicationQuorumNotReached),
            0x0007_0001 => Some(Self::TemporaryUnavailable),
            0x0007_0002 => Some(Self::StorageExhausted),
            0x0008_0001 => Some(Self::Internal),
            _ => None,
        }
    }

    /// Reserved-range class of this code (contract table 6).
    pub const fn class(self) -> ResultClass {
        match self {
            Self::UnknownOperation
            | Self::InvalidArguments
            | Self::InvalidRequestState
            | Self::RepeatedRequestId => ResultClass::Request,
            Self::AuthenticationRejected => ResultClass::Authentication,
            Self::RegistrationConflict | Self::RegistrationRejected => ResultClass::Registration,
            Self::InvalidCursor | Self::ExpiredSnapshot => ResultClass::Catalog,
            Self::FileSizeExceeded
            | Self::SizeMismatch
            | Self::InvalidFrameSequence
            | Self::UploadIdConflict => ResultClass::File,
            Self::QuorumUnavailable | Self::ReplicationQuorumNotReached => ResultClass::Quorum,
            Self::TemporaryUnavailable | Self::StorageExhausted => ResultClass::Resource,
            Self::Internal => ResultClass::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ResultCode; 18] = [
        ResultCode::UnknownOperation,
        ResultCode::InvalidArguments,
        ResultCode::InvalidRequestState,
        ResultCode::RepeatedRequestId,
        ResultCode::AuthenticationRejected,
        ResultCode::RegistrationConflict,
        ResultCode::RegistrationRejected,
        ResultCode::InvalidCursor,
        ResultCode::ExpiredSnapshot,
        ResultCode::FileSizeExceeded,
        ResultCode::SizeMismatch,
        ResultCode::InvalidFrameSequence,
        ResultCode::UploadIdConflict,
        ResultCode::QuorumUnavailable,
        ResultCode::ReplicationQuorumNotReached,
        ResultCode::TemporaryUnavailable,
        ResultCode::StorageExhausted,
        ResultCode::Internal,
    ];

    #[test]
    fn every_published_code_carries_the_contract_value() {
        let expected = [
            (ResultCode::UnknownOperation, 0x0001_0001),
            (ResultCode::InvalidArguments, 0x0001_0002),
            (ResultCode::InvalidRequestState, 0x0001_0003),
            (ResultCode::RepeatedRequestId, 0x0001_0004),
            (ResultCode::AuthenticationRejected, 0x0002_0001),
            (ResultCode::RegistrationConflict, 0x0003_0001),
            (ResultCode::RegistrationRejected, 0x0003_0002),
            (ResultCode::InvalidCursor, 0x0004_0001),
            (ResultCode::ExpiredSnapshot, 0x0004_0002),
            (ResultCode::FileSizeExceeded, 0x0005_0001),
            (ResultCode::SizeMismatch, 0x0005_0002),
            (ResultCode::InvalidFrameSequence, 0x0005_0003),
            (ResultCode::UploadIdConflict, 0x0005_0004),
            (ResultCode::QuorumUnavailable, 0x0006_0001),
            (ResultCode::ReplicationQuorumNotReached, 0x0006_0002),
            (ResultCode::TemporaryUnavailable, 0x0007_0001),
            (ResultCode::StorageExhausted, 0x0007_0002),
            (ResultCode::Internal, 0x0008_0001),
        ];

        for (code, value) in expected {
            assert_eq!(code.as_u32(), value);
            assert_eq!(ResultCode::from_u32(value), Some(code));
        }
    }

    #[test]
    fn every_published_code_is_unique() {
        let mut values: Vec<u32> = ALL.iter().map(|code| code.as_u32()).collect();
        values.sort_unstable();
        values.dedup();
        assert_eq!(values.len(), ALL.len());
    }

    #[test]
    fn each_code_belongs_to_its_reserved_range() {
        assert_eq!(ResultCode::UnknownOperation.class(), ResultClass::Request);
        assert_eq!(
            ResultCode::AuthenticationRejected.class(),
            ResultClass::Authentication
        );
        assert_eq!(
            ResultCode::RegistrationConflict.class(),
            ResultClass::Registration
        );
        assert_eq!(ResultCode::InvalidCursor.class(), ResultClass::Catalog);
        assert_eq!(ResultCode::FileSizeExceeded.class(), ResultClass::File);
        assert_eq!(ResultCode::QuorumUnavailable.class(), ResultClass::Quorum);
        assert_eq!(
            ResultCode::TemporaryUnavailable.class(),
            ResultClass::Resource
        );
        assert_eq!(ResultCode::Internal.class(), ResultClass::Internal);
        for code in ALL {
            assert_eq!(
                ResultClass::from_u32(code.as_u32()),
                Some(code.class()),
                "{code:?}"
            );
        }
    }

    #[test]
    fn unlisted_values_inside_a_range_are_not_published_codes() {
        assert_eq!(ResultCode::from_u32(0x0001_0005), None);
        assert_eq!(ResultCode::from_u32(0x0004_FFFF), None);
        assert_eq!(
            ResultClass::from_u32(0x0004_FFFF),
            Some(ResultClass::Catalog)
        );
    }

    #[test]
    fn success_and_unreserved_values_have_no_class() {
        assert_eq!(ResultClass::from_u32(0), None);
        assert_eq!(ResultClass::from_u32(0x0000_FFFF), None);
        assert_eq!(ResultClass::from_u32(0x0009_0000), None);
        assert_eq!(ResultClass::from_u32(u32::MAX), None);
    }

    #[test]
    fn reserved_range_boundaries_map_to_the_documented_class() {
        let boundaries = [
            (0x0001_0000, ResultClass::Request),
            (0x0001_FFFF, ResultClass::Request),
            (0x0002_0000, ResultClass::Authentication),
            (0x0003_FFFF, ResultClass::Registration),
            (0x0004_0000, ResultClass::Catalog),
            (0x0005_FFFF, ResultClass::File),
            (0x0006_0000, ResultClass::Quorum),
            (0x0007_FFFF, ResultClass::Resource),
            (0x0008_0000, ResultClass::Internal),
            (0x0008_FFFF, ResultClass::Internal),
        ];

        for (value, class) in boundaries {
            assert_eq!(ResultClass::from_u32(value), Some(class), "{value:#010x}");
        }
    }
}
