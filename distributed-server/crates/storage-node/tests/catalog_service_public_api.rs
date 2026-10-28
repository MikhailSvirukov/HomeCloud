//! Integration tests for the public committed-catalog service.
//!
//! Covers `FR-CAT-007 / 1`: committed versions are served in ascending upload id
//! order. Covers `FR-CAT-008 / 1`: every page of one snapshot carries the same
//! snapshot token. Covers `FR-CAT-009 / 1` and `FR-CAT-014 / 2`: cursors are
//! opaque, bound to their snapshot, and rejected after the snapshot expires.
//! Covers `FR-CAT-011 / 1`: page size, snapshot lifetime, and the source-name
//! limit are configured through the constructor. Covers `FR-CAT-012 / 3` and
//! `FR-CAT-013 / 2`: entries preserve the committed values and pages expose a
//! token, cursor, and entries. Covers `FR-NODE-005 / 1`–`FR-NODE-008 / 1`: each
//! catalog entry carries the upload id, SHA-256, original name, and capture time.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::Duration;

use storage_node::CatalogEntry;
use storage_node::CatalogError;
use storage_node::CatalogPagerError;
use storage_node::CatalogService;
use storage_node::CatalogServiceError;
use storage_node::CommittedVersion;
use storage_node::ContentHash;
use storage_node::UploadId;
use storage_node::UploadRecord;
use storage_node::CATALOG_TOKEN_LEN;

/// Source-name byte limit used by tests that expect accepted metadata.
const MAX_SOURCE_NAME_LEN: usize = 64;

/// Snapshot lifetime used by tests that expect a live snapshot.
const LIVE_SNAPSHOT_TTL: Duration = Duration::from_secs(30);

fn version(id: u8, name: &str) -> CommittedVersion {
    CommittedVersion {
        record: UploadRecord::new(
            UploadId::from_bytes([id; 16]),
            "alice",
            name,
            100,
            40,
            ContentHash::digest(&[id]),
            1_700_000_000_000,
            1_700_000_100_000,
        )
        .expect("valid metadata"),
        replicas: [1, 2, 3].into_iter().collect(),
    }
}

fn service(page_size: usize, snapshot_ttl: Duration) -> CatalogService {
    let page_size = NonZeroUsize::new(page_size).expect("the test page size is non-zero");
    let max_source_name_len =
        NonZeroUsize::new(MAX_SOURCE_NAME_LEN).expect("the test source-name limit is non-zero");
    CatalogService::new(page_size, snapshot_ttl, max_source_name_len)
}

fn catalog(ids: &[u8]) -> BTreeMap<UploadId, CommittedVersion> {
    let mut versions = BTreeMap::new();
    for id in ids {
        let version = version(*id, &format!("photo-{id}.jpg"));
        versions.insert(version.record.upload_id, version);
    }
    versions
}

fn ids(entries: &[CatalogEntry]) -> Vec<u8> {
    entries
        .iter()
        .map(|entry| entry.upload_id.as_bytes()[0])
        .collect()
}

#[test]
fn serves_the_committed_catalog_in_ascending_order() {
    let versions = catalog(&[2, 1]);
    let mut service = service(2, LIVE_SNAPSHOT_TTL);

    let page = service
        .first_page(&versions)
        .expect("the committed catalog yields a first page");

    assert_eq!(ids(&page.entries), [1, 2]);
    assert_eq!(page.next_cursor, None);
    assert_eq!(page.snapshot_token.len(), CATALOG_TOKEN_LEN);

    for entry in &page.entries {
        let committed = versions
            .get(&entry.upload_id)
            .expect("each entry has a committed version");
        assert_eq!(entry.source_name, committed.record.original_name);
        assert_eq!(
            entry.timestamp_ms,
            i64::try_from(committed.record.captured_at_ms)
                .expect("the capture time fits milliseconds")
        );
        assert_eq!(entry.original_size_bytes, committed.record.size_bytes);
        assert_eq!(
            entry.compressed_size_bytes,
            committed.record.compressed_size_bytes
        );
        assert_eq!(entry.sha256, committed.record.content_hash);
    }
}

#[test]
fn continues_the_snapshot_under_the_same_token() {
    let versions = catalog(&[1, 2, 3]);
    let mut service = service(2, LIVE_SNAPSHOT_TTL);

    let first = service
        .first_page(&versions)
        .expect("the committed catalog yields a first page");
    assert_eq!(ids(&first.entries), [1, 2]);
    let cursor = first
        .next_cursor
        .expect("the first page of a three-entry catalog continues");

    let second = service
        .next_page(first.snapshot_token, cursor)
        .expect("the cursor belongs to the first page snapshot");

    assert_eq!(ids(&second.entries), [3]);
    assert_eq!(second.next_cursor, None);
    assert_eq!(second.snapshot_token, first.snapshot_token);
}

#[test]
fn rejects_a_source_name_over_the_configured_limit() {
    let long_name = "a".repeat(MAX_SOURCE_NAME_LEN + 1);
    let versions = BTreeMap::from([(UploadId::from_bytes([1; 16]), version(1, &long_name))]);
    let mut service = service(1, LIVE_SNAPSHOT_TTL);

    let error = service
        .first_page(&versions)
        .expect_err("a source name over the configured limit is rejected");

    assert!(matches!(
        error,
        CatalogServiceError::Entry(CatalogError::SourceNameTooLong)
    ));
}

#[test]
fn rejects_a_continuation_after_snapshot_expiry() {
    let versions = catalog(&[1, 2]);
    let mut service = service(1, Duration::ZERO);

    let first = service
        .first_page(&versions)
        .expect("the first page is created before the snapshot expires");
    let cursor = first
        .next_cursor
        .expect("the first page of a two-entry catalog continues");

    let error = service
        .next_page(first.snapshot_token, cursor)
        .expect_err("a zero snapshot lifetime expires continuations");

    assert!(matches!(
        error,
        CatalogServiceError::Pager(CatalogPagerError::SnapshotExpired)
    ));
}
