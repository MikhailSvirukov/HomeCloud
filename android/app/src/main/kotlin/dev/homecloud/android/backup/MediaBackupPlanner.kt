package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.queue.UploadQueue

/**
 * Outcome of a single [MediaBackupPlanner.plan] run.
 *
 * [newlyEnqueued] counts only candidates first seen in that run: ids already tracked, or repeated
 * within the run, are excluded.
 */
internal data class PlanResult(
    val discoveredCandidates: Int,
    val newlyEnqueued: Int,
)

/**
 * Coordinates media discovery with the upload queue: [plan] records candidates not yet tracked as
 * pending work. Pure domain coordination, with no persistence, scheduling, I/O, or coroutines; the
 * upload identity policy is delegated to the injected [uploadIdFactory].
 */
internal class MediaBackupPlanner(
    private val discovery: MediaDiscovery,
    private val uploadIdFactory: UploadIdFactory,
    val queue: UploadQueue,
) {

    /**
     * Enqueues each discovered candidate whose upload id is not already tracked. Discovery is
     * queried once and the tracked ids are snapshotted before enqueuing, so two candidates mapping to
     * the same id produce exactly one entry and a re-run after a successful pass enqueues nothing.
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
