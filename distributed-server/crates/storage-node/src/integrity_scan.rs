//! Periodic integrity scanning of committed blobs.
//!
//! SRS: `FR-CL-013 / 2`, `CFG-CL-037 / 1`, supporting `FR-CL-021 / 2`.

use core::num::{NonZeroU64, NonZeroUsize};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;

use crate::integrity::{verify_local_blob, BlobIntegrity, IntegrityError};
use crate::metadata::UploadId;
use crate::raft::CommittedVersion;
use crate::storage_layout::StorageLayout;

/// Schedule and concurrency limit of the periodic integrity scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrityScanConfig {
    /// Delay between full scans of committed blobs.
    pub period: Duration,
    /// Maximum number of blobs verified at the same time.
    pub max_concurrent: NonZeroUsize,
}

impl IntegrityScanConfig {
    /// Builds a scan configuration from a non-zero period in seconds and a concurrency limit.
    pub fn new(period_seconds: NonZeroU64, max_concurrent: NonZeroUsize) -> Self {
        Self {
            period: Duration::from_secs(period_seconds.get()),
            max_concurrent,
        }
    }
}

/// Outcome of one scan over the committed blobs held by this node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrityScanReport {
    /// Blobs whose digest was compared, both healthy and corrupted.
    pub checked: usize,
    /// Upload ids whose local bytes differ from the committed digest.
    pub corrupted: Vec<UploadId>,
    /// Upload ids whose local blob file is absent.
    pub missing: Vec<UploadId>,
    /// Read, marker, permit, or task-join failures.
    pub errors: usize,
}

/// Verifies every committed blob in parallel up to the configured limit.
pub async fn scan_committed_blobs(
    layout: &StorageLayout,
    versions: Vec<CommittedVersion>,
    max_concurrent: NonZeroUsize,
) -> IntegrityScanReport {
    let semaphore = Arc::new(Semaphore::new(max_concurrent.get()));
    let mut tasks = JoinSet::new();
    for version in versions {
        let layout = layout.clone();
        let permits = Arc::clone(&semaphore);
        let upload_id = version.record.upload_id;
        tasks.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return (upload_id, None);
            };
            let result =
                tokio::task::spawn_blocking(move || verify_local_blob(&layout, &version.record))
                    .await
                    .ok();
            (upload_id, result)
        });
    }

    let mut report = IntegrityScanReport::default();
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_, Some(Ok(BlobIntegrity::Healthy)))) => report.checked += 1,
            Ok((upload_id, Some(Ok(BlobIntegrity::Corrupted)))) => {
                report.checked += 1;
                report.corrupted.push(upload_id);
            }
            Ok((upload_id, Some(Err(IntegrityError::Missing(_))))) => {
                report.missing.push(upload_id);
            }
            Ok((_, Some(Err(_)))) | Ok((_, None)) | Err(_) => report.errors += 1,
        }
    }
    report.corrupted.sort_unstable();
    report.missing.sort_unstable();
    report
}

/// Runs the integrity scan on the configured period until `shutdown` resolves, reporting each scan.
pub async fn run_integrity_scan_loop<F, S, R>(
    layout: StorageLayout,
    source: S,
    config: IntegrityScanConfig,
    shutdown: F,
    mut on_report: R,
) where
    F: Future<Output = ()>,
    S: Fn() -> Vec<CommittedVersion>,
    R: FnMut(IntegrityScanReport),
{
    let mut interval = tokio::time::interval(config.period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut shutdown = core::pin::pin!(shutdown);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let versions = source();
                let report = scan_committed_blobs(&layout, versions, config.max_concurrent).await;
                on_report(report);
            }
            _ = &mut shutdown => return,
        }
    }
}
