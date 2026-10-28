//! CSPRNG-backed catalog pagination snapshots.
//!
//! SRS: `FR-CAT-007 / 1`, `FR-CAT-008 / 1`, `FR-CAT-009 / 1`,
//! `FR-CAT-011 / 1`, `FR-CAT-014 / 2`.

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::catalog::{CatalogEntry, CatalogPage, CATALOG_TOKEN_LEN};

/// Maximum CSPRNG draws attempted for one collision-free token.
const MAX_TOKEN_ATTEMPTS: usize = 16;

/// Opaque pagination token carried by a snapshot or cursor.
type PaginationToken = [u8; CATALOG_TOKEN_LEN];

/// Cursors issued for one snapshot.
type CursorStates = HashMap<PaginationToken, CursorState>;

/// Cursor position and the continuation cursor that follows it.
struct CursorState {
    /// Index of the first entry on the cursor's page.
    offset: usize,
    /// Cursor for the following page, or `None` on the last page.
    next_cursor: Option<PaginationToken>,
}

/// One committed catalog snapshot served across several pages.
struct Snapshot {
    /// Entries in ascending `upload_id` order.
    entries: Arc<[CatalogEntry]>,
    /// Instant at which the snapshot was created.
    created_at: Instant,
    /// Cursors issued for this snapshot.
    cursors: CursorStates,
}

/// Failure modes when paging a catalog snapshot.
#[derive(Debug, thiserror::Error)]
pub enum CatalogPagerError {
    /// The snapshot lifetime elapsed before the page was requested.
    #[error("the catalog snapshot has expired")]
    SnapshotExpired,
    /// No snapshot exists for the supplied token.
    #[error("the catalog snapshot is unknown")]
    UnknownSnapshot,
    /// The cursor does not belong to the supplied snapshot.
    #[error("the catalog cursor is unknown")]
    UnknownCursor,
    /// The cursor belongs to a different snapshot.
    #[error("the cursor belongs to a different catalog snapshot")]
    CursorSnapshotMismatch,
    /// The CSPRNG did not produce a usable token.
    #[error("the pagination token could not be generated")]
    Random,
}

/// Paginates committed catalog snapshots with CSPRNG-backed tokens.
pub struct CatalogPager {
    page_size: NonZeroUsize,
    snapshot_ttl: Duration,
    snapshots: HashMap<PaginationToken, Snapshot>,
}

impl fmt::Debug for CatalogPager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogPager")
            .field("snapshots", &self.snapshots.len())
            .field("page_size", &self.page_size)
            .field("snapshot_ttl", &self.snapshot_ttl)
            .finish()
    }
}

impl CatalogPager {
    /// Creates a pager with the configured page size and snapshot lifetime.
    pub fn new(page_size: NonZeroUsize, snapshot_ttl: Duration) -> Self {
        Self {
            page_size,
            snapshot_ttl,
            snapshots: HashMap::new(),
        }
    }

    /// Sorts the committed entries by upload id, starts a snapshot, and returns its first page.
    pub fn first_page(
        &mut self,
        mut entries: Vec<CatalogEntry>,
    ) -> Result<CatalogPage, CatalogPagerError> {
        entries.sort_by_key(|entry| entry.upload_id);

        let snapshot_token = generate_token(|token| self.snapshots.contains_key(token))?;
        let entries: Arc<[CatalogEntry]> = entries.into();
        let page_size = self.page_size.get();
        let first_page_len = page_size.min(entries.len());
        let first_entries = entries
            .get(..first_page_len)
            .map_or_else(Vec::new, <[CatalogEntry]>::to_vec);
        let (cursors, first_cursor) = build_cursors(page_size, entries.len())?;

        self.snapshots.insert(
            snapshot_token,
            Snapshot {
                entries,
                created_at: Instant::now(),
                cursors,
            },
        );

        Ok(CatalogPage {
            snapshot_token,
            next_cursor: first_cursor,
            entries: first_entries,
        })
    }

    /// Returns the page addressed by `cursor` within the snapshot `snapshot_token`.
    pub fn next_page(
        &mut self,
        snapshot_token: [u8; CATALOG_TOKEN_LEN],
        cursor: [u8; CATALOG_TOKEN_LEN],
    ) -> Result<CatalogPage, CatalogPagerError> {
        let Some(snapshot) = self.snapshots.get(&snapshot_token) else {
            return Err(CatalogPagerError::UnknownSnapshot);
        };
        if snapshot.created_at.elapsed() >= self.snapshot_ttl {
            return Err(CatalogPagerError::SnapshotExpired);
        }
        let Some(cursor_state) = snapshot.cursors.get(&cursor) else {
            if self.snapshots.iter().any(|(token, other)| {
                *token != snapshot_token && other.cursors.contains_key(&cursor)
            }) {
                return Err(CatalogPagerError::CursorSnapshotMismatch);
            }
            return Err(CatalogPagerError::UnknownCursor);
        };

        let start = cursor_state.offset.min(snapshot.entries.len());
        let end = start
            .saturating_add(self.page_size.get())
            .min(snapshot.entries.len());

        Ok(CatalogPage {
            snapshot_token,
            next_cursor: cursor_state.next_cursor,
            entries: snapshot
                .entries
                .get(start..end)
                .map_or_else(Vec::new, <[CatalogEntry]>::to_vec),
        })
    }
}

fn build_cursors(
    page_size: usize,
    entry_count: usize,
) -> Result<(CursorStates, Option<PaginationToken>), CatalogPagerError> {
    let mut cursors = HashMap::new();
    let mut next_cursor = None;

    for offset in (page_size..entry_count).step_by(page_size).rev() {
        let cursor = generate_token(|token| cursors.contains_key(token))?;
        cursors.insert(
            cursor,
            CursorState {
                offset,
                next_cursor,
            },
        );
        next_cursor = Some(cursor);
    }

    Ok((cursors, next_cursor))
}

fn generate_token(
    is_taken: impl Fn(&PaginationToken) -> bool,
) -> Result<PaginationToken, CatalogPagerError> {
    for _ in 0..MAX_TOKEN_ATTEMPTS {
        let mut token = [0_u8; CATALOG_TOKEN_LEN];
        getrandom::fill(&mut token).map_err(|_| CatalogPagerError::Random)?;
        if !is_taken(&token) {
            return Ok(token);
        }
    }
    Err(CatalogPagerError::Random)
}
