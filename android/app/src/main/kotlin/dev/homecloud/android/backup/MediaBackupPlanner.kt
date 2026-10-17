package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaVersionScanner
import dev.homecloud.android.media.newVersionUploadId
import dev.homecloud.android.queue.UploadQueue

/**
 * Outcome of a single [MediaBackupPlanner.plan] run.
 *
 * [newlyEnqueued] counts only versions not tracked before that run: candidates whose identity is
 * already queued, or repeated within the run, are excluded.
 */
internal data class PlanResult(
    val discoveredCandidates: Int,
    val newlyEnqueued: Int,
)

/**
 * Coordinates media discovery with the upload queue: [plan] records versions not yet tracked as
 * pending work. The version identity decides whether a candidate is new, so an unchanged candidate
 * is left alone and a changed one receives a new upload id. Pure domain coordination, with no
 * persistence, scheduling, I/O, or coroutines.
 */
internal class MediaBackupPlanner(
    private val scanner: MediaVersionScanner,
    val queue: UploadQueue,
) {
    /**
     * Enqueues each scanned candidate whose version identity is not already tracked. The scan runs
     * once and the tracked identities are snapshotted before enqueuing, so two candidates carrying
     * the same identity produce exactly one entry and a re-run over an unchanged media store
     * enqueues nothing.
     */
    fun plan(): PlanResult {
        val candidates = scanner.scan()
        val trackedIdentities = queue.entries().map { entry -> entry.identity }.toSet()

        val newVersions =
            candidates
                .filter { candidate -> candidate.identity !in trackedIdentities }
                .distinctBy { candidate -> candidate.identity }

        newVersions.forEach { candidate ->
            queue.enqueue(
                uploadId = newVersionUploadId(),
                sourceId = candidate.item.id,
                identity = candidate.identity,
                sizeBytes = candidate.item.sizeBytes,
            )
        }

        return PlanResult(
            discoveredCandidates = candidates.size,
            newlyEnqueued = newVersions.size,
        )
    }
}
