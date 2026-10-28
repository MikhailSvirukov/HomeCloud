//! Integration tests for the public catalog error codes.
//!
//! Covers `INT-NET-064 / 1`: catalog failures carry the protocol codes
//! `0x0004_0001` for an invalid cursor, `0x0004_0002` for an expired snapshot,
//! and `0x0008_0001` for an internal error. Supporting `FR-CAT-009 / 1`: a
//! cursor that is unknown or belongs to another snapshot maps to the invalid
//! cursor code. Supporting `FR-CAT-014 / 2`: an expired or unknown snapshot
//! maps to the expired snapshot code.

use storage_node::CatalogError;
use storage_node::CatalogErrorCode;
use storage_node::CatalogPagerError;
use storage_node::CatalogServiceError;

fn assert_maps_to(error: impl Into<CatalogErrorCode>, expected: CatalogErrorCode) {
    assert_eq!(error.into(), expected);
}

#[test]
fn wire_codes_match_the_protocol_table() {
    assert_eq!(u32::from(CatalogErrorCode::InvalidCursor), 0x0004_0001);
    assert_eq!(u32::from(CatalogErrorCode::SnapshotExpired), 0x0004_0002);
    assert_eq!(u32::from(CatalogErrorCode::Internal), 0x0008_0001);
}

#[test]
fn maps_all_pager_failures() {
    assert_maps_to(
        CatalogPagerError::SnapshotExpired,
        CatalogErrorCode::SnapshotExpired,
    );
    assert_maps_to(
        CatalogPagerError::UnknownSnapshot,
        CatalogErrorCode::SnapshotExpired,
    );
    assert_maps_to(
        CatalogPagerError::UnknownCursor,
        CatalogErrorCode::InvalidCursor,
    );
    assert_maps_to(
        CatalogPagerError::CursorSnapshotMismatch,
        CatalogErrorCode::InvalidCursor,
    );
    assert_maps_to(CatalogPagerError::Random, CatalogErrorCode::Internal);
}

#[test]
fn maps_service_failures() {
    assert_maps_to(
        CatalogServiceError::Pager(CatalogPagerError::SnapshotExpired),
        CatalogErrorCode::SnapshotExpired,
    );
    assert_maps_to(
        CatalogServiceError::Entry(CatalogError::SourceNameTooLong),
        CatalogErrorCode::Internal,
    );
    assert_maps_to(
        CatalogServiceError::Pager(CatalogPagerError::UnknownCursor),
        CatalogErrorCode::InvalidCursor,
    );
}
