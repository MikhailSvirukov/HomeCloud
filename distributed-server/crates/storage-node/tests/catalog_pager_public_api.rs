//! Integration tests for the public catalog pager.
//!
//! Covers `FR-CAT-007 / 1`: pages follow ascending `upload_id` order. Covers
//! `FR-CAT-008 / 1`: every page of one snapshot carries the same snapshot token.
//! Covers `FR-CAT-009 / 1`: a cursor is opaque and only valid together with its
//! snapshot token. Covers `FR-CAT-011 / 1`: page size and snapshot lifetime are
//! configured through the constructor. Covers `FR-CAT-014 / 2`: snapshot tokens
//! and cursors are independently generated random values, and a repeated cursor
//! maps back to the same next page and snapshot.

use std::num::NonZeroUsize;
use std::time::Duration;

use storage_node::CatalogEntry;
use storage_node::CatalogPage;
use storage_node::CatalogPager;
use storage_node::CatalogPagerError;
use storage_node::ContentHash;
use storage_node::UploadId;
use storage_node::CATALOG_TOKEN_LEN;

/// Source-name byte limit used by the test entries.
const MAX_SOURCE_NAME_LEN: usize = 64;

/// Snapshot lifetime used by tests that expect a live snapshot.
const LIVE_SNAPSHOT_TTL: Duration = Duration::from_secs(30);

fn entry(id: u8) -> CatalogEntry {
    CatalogEntry::new(
        UploadId::from_bytes([id; 16]),
        "photo.jpg",
        0,
        10,
        5,
        ContentHash::digest(&[id]),
        MAX_SOURCE_NAME_LEN,
    )
    .expect("the fixed source name fits the limit")
}

fn pager(page_size: usize, snapshot_ttl: Duration) -> CatalogPager {
    let page_size = NonZeroUsize::new(page_size).expect("the test page size is non-zero");
    CatalogPager::new(page_size, snapshot_ttl)
}

fn ids(entries: &[CatalogEntry]) -> Vec<u8> {
    entries
        .iter()
        .map(|entry| entry.upload_id.as_bytes()[0])
        .collect()
}

fn assert_no_cursor(page: &CatalogPage) {
    assert_eq!(page.next_cursor, None);
}

#[test]
fn pages_follow_ascending_upload_id_order() {
    let mut pager = pager(2, LIVE_SNAPSHOT_TTL);

    let first = pager
        .first_page(vec![entry(3), entry(1), entry(4), entry(2)])
        .expect("the first page is created from any catalog");
    assert_eq!(ids(&first.entries), [1, 2]);
    let cursor = first
        .next_cursor
        .expect("the first page of a four-entry catalog continues");

    let second = pager
        .next_page(first.snapshot_token, cursor)
        .expect("the cursor belongs to the first page snapshot");
    assert_eq!(ids(&second.entries), [3, 4]);
    assert_no_cursor(&second);
    assert_eq!(second.snapshot_token, first.snapshot_token);
}

#[test]
fn last_page_has_no_cursor() {
    let mut pager = pager(4, LIVE_SNAPSHOT_TTL);

    let page = pager
        .first_page(vec![entry(1), entry(2), entry(3), entry(4)])
        .expect("the only page is created from any catalog");
    assert_eq!(ids(&page.entries), [1, 2, 3, 4]);
    assert_no_cursor(&page);
}

#[test]
fn empty_catalog_has_a_token_and_no_cursor() {
    let mut pager = pager(2, LIVE_SNAPSHOT_TTL);

    let page = pager
        .first_page(Vec::new())
        .expect("an empty catalog still yields a first page");
    assert!(page.entries.is_empty());
    assert_no_cursor(&page);
    assert_eq!(page.snapshot_token.len(), CATALOG_TOKEN_LEN);
}

#[test]
fn cursor_is_bound_to_its_snapshot() {
    let mut pager_a = pager(1, LIVE_SNAPSHOT_TTL);
    let mut pager_b = pager(1, LIVE_SNAPSHOT_TTL);
    let page_a = pager_a
        .first_page(vec![entry(1), entry(2)])
        .expect("snapshot A is created");
    let page_b = pager_b
        .first_page(vec![entry(3), entry(4)])
        .expect("snapshot B is created");
    let cursor_a = page_a
        .next_cursor
        .expect("snapshot A continues past its first entry");

    let error = pager_b
        .next_page(page_b.snapshot_token, cursor_a)
        .expect_err("a cursor is rejected under another snapshot token");
    assert!(matches!(error, CatalogPagerError::CursorSnapshotMismatch));

    pager_a
        .next_page(page_a.snapshot_token, cursor_a)
        .expect("the cursor is accepted under its own snapshot token");
}

#[test]
fn unknown_snapshot_and_cursor_are_rejected() {
    let mut pager = pager(1, LIVE_SNAPSHOT_TTL);
    let page = pager
        .first_page(vec![entry(1), entry(2)])
        .expect("the first page is created");

    let unknown_snapshot = pager
        .next_page([0x7f; CATALOG_TOKEN_LEN], [0x11; CATALOG_TOKEN_LEN])
        .expect_err("an unknown snapshot token is rejected");
    assert!(matches!(
        unknown_snapshot,
        CatalogPagerError::UnknownSnapshot
    ));

    let unknown_cursor = pager
        .next_page(page.snapshot_token, [0x7e; CATALOG_TOKEN_LEN])
        .expect_err("an unknown cursor is rejected");
    assert!(matches!(unknown_cursor, CatalogPagerError::UnknownCursor));
}

#[test]
fn expired_snapshot_is_rejected() {
    let mut pager = pager(1, Duration::ZERO);
    let page = pager
        .first_page(vec![entry(1), entry(2)])
        .expect("the first page is created before the snapshot expires");
    let cursor = page
        .next_cursor
        .expect("the first page of a two-entry catalog continues");

    let error = pager
        .next_page(page.snapshot_token, cursor)
        .expect_err("a zero snapshot lifetime expires continuations");
    assert!(matches!(error, CatalogPagerError::SnapshotExpired));
}

#[test]
fn tokens_are_random_and_distinct() {
    let mut pager_a = pager(1, LIVE_SNAPSHOT_TTL);
    let mut pager_b = pager(1, LIVE_SNAPSHOT_TTL);
    let page_a = pager_a
        .first_page(vec![entry(1), entry(2), entry(3)])
        .expect("snapshot A is created");
    let page_b = pager_b
        .first_page(vec![entry(1), entry(2), entry(3)])
        .expect("snapshot B is created");
    assert_ne!(page_a.snapshot_token, page_b.snapshot_token);

    let cursor_a = page_a
        .next_cursor
        .expect("snapshot A continues past its first entry");
    let second = pager_a
        .next_page(page_a.snapshot_token, cursor_a)
        .expect("the first cursor belongs to snapshot A");
    let cursor_b = second
        .next_cursor
        .expect("snapshot A continues past its second entry");
    assert_ne!(cursor_a, cursor_b);
}

#[test]
fn cursor_reuse_returns_the_same_page_and_next_cursor() {
    let mut pager = pager(1, LIVE_SNAPSHOT_TTL);
    let first = pager
        .first_page(vec![entry(1), entry(2), entry(3)])
        .expect("the first page is created");
    let cursor = first
        .next_cursor
        .expect("the first page continues past its first entry");

    let page_a = pager
        .next_page(first.snapshot_token, cursor)
        .expect("the cursor is accepted the first time");
    let page_b = pager
        .next_page(first.snapshot_token, cursor)
        .expect("the cursor is accepted again for the same snapshot");
    assert_eq!(ids(&page_a.entries), ids(&page_b.entries));
    assert_eq!(page_a.next_cursor, page_b.next_cursor);
    assert_eq!(page_a.snapshot_token, page_b.snapshot_token);
}
