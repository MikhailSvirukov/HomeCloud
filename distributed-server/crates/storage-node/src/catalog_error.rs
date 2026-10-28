//! Protocol error codes for catalog page requests.
//!
//! SRS: `INT-NET-064 / 1`, supporting `FR-CAT-009 / 1`, `FR-CAT-014 / 2`.

use crate::catalog_pager::CatalogPagerError;
use crate::catalog_service::CatalogServiceError;

/// Protocol error code for a failed catalog page request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CatalogErrorCode {
    /// The cursor is invalid for this snapshot.
    InvalidCursor = 0x0004_0001,
    /// The snapshot is unknown or expired; the client must restart the catalog read.
    SnapshotExpired = 0x0004_0002,
    /// An unexpected server failure.
    Internal = 0x0008_0001,
}

impl From<CatalogErrorCode> for u32 {
    fn from(code: CatalogErrorCode) -> Self {
        code as Self
    }
}

impl From<CatalogPagerError> for CatalogErrorCode {
    fn from(error: CatalogPagerError) -> Self {
        match error {
            CatalogPagerError::SnapshotExpired | CatalogPagerError::UnknownSnapshot => {
                Self::SnapshotExpired
            }
            CatalogPagerError::UnknownCursor | CatalogPagerError::CursorSnapshotMismatch => {
                Self::InvalidCursor
            }
            CatalogPagerError::Random => Self::Internal,
        }
    }
}

impl From<CatalogServiceError> for CatalogErrorCode {
    fn from(error: CatalogServiceError) -> Self {
        match error {
            CatalogServiceError::Pager(error) => error.into(),
            CatalogServiceError::Entry(_) => Self::Internal,
        }
    }
}
