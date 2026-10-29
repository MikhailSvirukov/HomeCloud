//! Integration tests exercising the public periodic blob-integrity scan.
//!
//! SRS: `FR-CL-013 / 2` (the node verifies every committed blob held locally on
//! a configured schedule), `CFG-CL-037 / 1` (the scan period and the maximum
//! number of concurrently checked blobs are configured), supporting
//! `FR-CL-021 / 2` (a digest mismatch marks the local copy corrupted).

use core::num::{NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::path::{Path, PathBuf};

use storage_node::{
    run_integrity_scan_loop, scan_committed_blobs, CommittedVersion, ContentHash,
    IntegrityScanConfig, StorageLayout, UploadId, UploadRecord, CORRUPT_MARKER_SUFFIX,
    UPLOAD_ID_LEN,
};
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};

const LOGIN: &str = "alice";
const ORIGINAL_NAME: &str = "photo.jpg";
const CAPTURED_AT_MS: u64 = 1_700_000_000_000;
const ACCEPTED_AT_MS: u64 = 1_700_000_001_000;

fn record(upload_id: u8, payload: &[u8]) -> UploadRecord {
    UploadRecord::new(
        UploadId::from_bytes([upload_id; UPLOAD_ID_LEN]),
        LOGIN,
        ORIGINAL_NAME,
        payload.len() as u64,
        payload.len() as u64 / 2,
        ContentHash::digest(payload),
        CAPTURED_AT_MS,
        ACCEPTED_AT_MS,
    )
    .expect("the test metadata is valid")
}

fn version(record: UploadRecord, replicas: [u64; 3]) -> CommittedVersion {
    CommittedVersion {
        record,
        replicas: replicas.into_iter().collect(),
    }
}

fn empty_layout() -> (TempDir, StorageLayout) {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    layout
        .ensure_dirs()
        .expect("layout directories are created");
    (temp, layout)
}

fn blob_path(layout: &StorageLayout, record: &UploadRecord) -> PathBuf {
    layout
        .blob_path(&record.login, *record.upload_id.as_bytes())
        .expect("record login and upload id form a valid blob path")
}

fn write_blob(layout: &StorageLayout, record: &UploadRecord, payload: &[u8]) -> PathBuf {
    let path = blob_path(layout, record);
    std::fs::create_dir_all(path.parent().expect("blob path has an account directory"))
        .expect("account directory is created");
    std::fs::write(&path, payload).expect("blob payload is written");
    path
}

fn marker_path(blob: &Path) -> PathBuf {
    let mut marker = blob.as_os_str().to_owned();
    marker.push(CORRUPT_MARKER_SUFFIX);
    PathBuf::from(marker)
}

fn flip_first_byte(payload: &[u8]) -> Vec<u8> {
    let mut corrupted = payload.to_vec();
    corrupted[0] ^= 0xFF;
    corrupted
}

fn concurrency(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("the test concurrency is non-zero")
}

#[tokio::test]
async fn scan_reports_healthy_corrupted_and_missing_blobs() {
    let (_temp, layout) = empty_layout();
    let healthy_payload = b"healthy committed blob";
    let corrupted_payload = b"corrupted committed blob";
    let missing_payload = b"missing committed blob";

    let healthy = version(record(1, healthy_payload), [1, 2, 3]);
    let corrupted = version(record(2, corrupted_payload), [1, 2, 3]);
    let missing = version(record(3, missing_payload), [1, 2, 3]);
    let corrupted_id = corrupted.record.upload_id;
    let missing_id = missing.record.upload_id;

    write_blob(&layout, &healthy.record, healthy_payload);
    let corrupted_blob = write_blob(&layout, &corrupted.record, corrupted_payload);
    std::fs::write(&corrupted_blob, flip_first_byte(corrupted_payload))
        .expect("the corrupted blob bytes are overwritten");

    let report =
        scan_committed_blobs(&layout, vec![healthy, corrupted, missing], concurrency(2)).await;

    assert_eq!(report.checked, 2, "both present blobs were digest-checked");
    assert_eq!(
        report.corrupted,
        [corrupted_id],
        "the mismatching blob is reported corrupted"
    );
    assert_eq!(
        report.missing,
        [missing_id],
        "the absent blob is reported missing"
    );
    assert_eq!(report.errors, 0, "no read or marker failure occurred");
    assert!(
        marker_path(&corrupted_blob).exists(),
        "a digest mismatch creates the corruption marker"
    );
}

#[tokio::test]
async fn scan_completes_with_a_single_concurrency_permit() {
    let (_temp, layout) = empty_layout();
    let payload = b"healthy committed blob";
    let versions: Vec<_> = (1..=4)
        .map(|upload_id| {
            let record = record(upload_id, payload);
            write_blob(&layout, &record, payload);
            version(record, [1, 2, 3])
        })
        .collect();

    let report = scan_committed_blobs(&layout, versions, concurrency(1)).await;

    assert_eq!(report.checked, 4, "every queued blob is checked");
    assert!(
        report.corrupted.is_empty(),
        "healthy blobs are not reported corrupted"
    );
    assert!(
        report.missing.is_empty(),
        "written blobs are not reported missing"
    );
    assert_eq!(report.errors, 0, "no scan task failed");
}

#[tokio::test]
async fn loop_reports_on_the_configured_period_and_stops_on_shutdown() {
    let (_temp, layout) = empty_layout();
    let payload = b"healthy committed blob";
    let record = record(1, payload);
    write_blob(&layout, &record, payload);
    let version = version(record, [1, 2, 3]);

    let period_seconds = NonZeroU64::new(1).expect("the test period is non-zero");
    let config = IntegrityScanConfig::new(period_seconds, concurrency(1));
    let (report_tx, mut report_rx) = mpsc::channel(1);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let scan = tokio::spawn(run_integrity_scan_loop(
        layout,
        move || vec![version.clone()],
        config,
        async move {
            let _ = shutdown_rx.await;
        },
        move |report| {
            let _ = report_tx.try_send(report);
        },
    ));

    let first = tokio::time::timeout(Duration::from_secs(3), report_rx.recv())
        .await
        .expect("the configured period delivers a report in time")
        .expect("the scan loop keeps its report sender alive");
    assert_eq!(first.checked, 1, "the report covers the committed blob");

    shutdown_tx
        .send(())
        .expect("the shutdown receiver stays alive until the loop stops");
    let outcome = scan.await;
    assert!(
        outcome.is_ok(),
        "the scan loop returns cleanly after shutdown"
    );
}
