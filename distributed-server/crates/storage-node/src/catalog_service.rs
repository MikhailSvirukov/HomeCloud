//! Catalog pages built from the committed Raft catalog.
//!
//! SRS: `FR-CAT-007 / 1`, `FR-CAT-008 / 1`, `FR-CAT-009 / 1`,
//! `FR-CAT-011 / 1`, `FR-CAT-012 / 3`, `FR-CAT-013 / 2`,
//! `FR-CAT-014 / 2`, `FR-NODE-005 / 1`–`FR-NODE-008 / 1`.

use core::fmt;
use core::num::NonZeroUsize;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::catalog::{CatalogEntry, CatalogError, CatalogPage};
use crate::catalog_pager::{CatalogPager, CatalogPagerError};
use crate::metadata::UploadId;
use crate::raft::CommittedVersion;

/// Serves paginated catalog pages from the committed version catalog.
pub struct CatalogService {
    pager: CatalogPager,
    max_source_name_len: NonZeroUsize,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogServiceError {
    /// A committed version cannot be represented as a catalog entry.
    #[error(transparent)]
    Entry(#[from] CatalogError),
    /// The pagination snapshot or cursor is invalid.
    #[error(transparent)]
    Pager(#[from] CatalogPagerError),
}

impl fmt::Debug for CatalogService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogService")
            .field("pager", &self.pager)
            .field("max_source_name_len", &self.max_source_name_len)
            .finish()
    }
}

impl CatalogService {
    /// Creates a service with the configured page size, snapshot lifetime, and source-name byte limit.
    pub fn new(
        page_size: NonZeroUsize,
        snapshot_ttl: Duration,
        max_source_name_len: NonZeroUsize,
    ) -> Self {
        Self {
            pager: CatalogPager::new(page_size, snapshot_ttl),
            max_source_name_len,
        }
    }

    /// Maps the committed versions to entries and returns the first page of a new snapshot.
    pub fn first_page(
        &mut self,
        versions: &BTreeMap<UploadId, CommittedVersion>,
    ) -> Result<CatalogPage, CatalogServiceError> {
        let entries = versions
            .values()
            .map(|version| CatalogEntry::from_committed(version, self.max_source_name_len.get()))
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Ok(self.pager.first_page(entries)?)
    }

    /// Returns the page addressed by `cursor` within the snapshot `snapshot_token`.
    pub fn next_page(
        &mut self,
        snapshot_token: [u8; crate::catalog::CATALOG_TOKEN_LEN],
        cursor: [u8; crate::catalog::CATALOG_TOKEN_LEN],
    ) -> Result<CatalogPage, CatalogServiceError> {
        Ok(self.pager.next_page(snapshot_token, cursor)?)
    }
}
