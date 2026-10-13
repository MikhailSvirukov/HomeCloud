package dev.picbackup.android.backup

import dev.picbackup.android.media.MediaDiscovery
import dev.picbackup.android.queue.UploadQueue

/**
 * Outcome of a single [MediaBackupPlanner.plan] run.
 *
 * @property discoveredCandidates number of photo/video candidates returned by discovery.
 * @property newlyEnqueued number of candidates newly enqueued this run. Candidates whose upload id
 *   was already tracked, or that repeated an id already enqueued in the same run, are not counted.
 */
data class PlanResult(
    val discoveredCandidates: Int,
    val newlyEnqueued: Int,
)

/**
 * Coordinates media discovery with the upload queue.
 *
 * [plan] reads the current backup candidates from [discovery] and records the ones not yet tracked
 * by [queue] as pending work. It is pure domain coordination: no persistence, scheduling, I/O, or
 * coroutines. The upload identity policy is delegated to the injected [uploadIdFactory].
 *
 * @param discovery source of the current backup candidates.
 * @param uploadIdFactory supplies the stable upload id for each candidate.
 * @param queue tracks the upload work to be performed.
 */
class MediaBackupPlanner(
    private val discovery: MediaDiscovery,
    private val uploadIdFactory: UploadIdFactory,
    private val queue: UploadQueue,
) {

    /**
     * Enqueues each discovered candidate whose upload id is not already tracked.
     *
     * [discovery] is queried exactly once and the upload ids already tracked by [queue] are
     * snapshotted before any enqueue. A candidate is enqueued only when its id is absent from that
     * snapshot and has not already been enqueued during this call, so two candidates mapping to the
     * same id produce exactly one entry. Existing entries are never reset or mutated; because the
     * queue's enqueue is idempotent, re-running [plan] after a successful first run enqueues
     * nothing.
     *
     * @return the candidate count and the number of newly enqueued entries.
     */
    fun plan(): PlanResult {
        val candidates = discovery.discoverBackupCandidates()
        val trackedUploadIds = queue.entries().map { entry -> entry.uploadId }.toSet()

        val newVersionEnqueues =
            candidates
                .map { item -> uploadIdFactory.uploadIdFor(item) to item }
                .distinctBy { (uploadId, _) -> uploadId }
                .filter { (uploadId, _) -> uploadId !in trackedUploadIds }

        newVersionEnqueues.forEach { (uploadId, item) ->
            queue.enqueue(
                uploadId = uploadId,
                sourceId = item.id,
                sizeBytes = item.sizeBytes,
                dateModifiedEpochSeconds = item.dateModifiedEpochSeconds,
            )
        }

        return PlanResult(
            discoveredCandidates = candidates.size,
            newlyEnqueued = newVersionEnqueues.size,
        )
    }
}
