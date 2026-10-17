package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaVersionScanner
import dev.homecloud.android.media.newVersionUploadId
import dev.homecloud.android.queue.UploadQueue

/**
 * Outcome of a single [MediaBackupPlanner.plan] run.
 *
 * [discoveredCandidates] counts every version the scan produced. [newlyEnqueued] counts only the
 * versions that run added: candidates whose identity was already tracked, repeats within the scan,
 * and candidates refused for capacity are excluded. [rejectedByCapacity] counts the distinct
 * untracked versions that were left out because the queue already held its configured maximum
 * (`CFG-AND-002 / 2`), so a full queue loses no accepted entry (`NFR-REL-001 / 1`).
 */
internal data class PlanResult(
    val discoveredCandidates: Int,
    val newlyEnqueued: Int,
    val rejectedByCapacity: Int,
)

/**
 * Coordinates media discovery with the upload queue: [plan] records versions not yet tracked as
 * pending work. The version identity decides whether a candidate is new, so an unchanged candidate
 * is left alone and a changed one receives a new upload id (`FR-AND-002 / 1`).
 *
 * The queue holds at most `maxQueueSize` versions (`CFG-AND-002 / 2`). Planning keeps every entry
 * already accepted: when the limit is reached, further candidates are reported as rejected by
 * capacity rather than enqueued, deleted, or replaced. A maximum below one is a configuration
 * error, never clamped or silently defaulted, so a misconfigured cycle does not start.
 *
 * Pure domain coordination, with no persistence, scheduling, I/O, or coroutines.
 */
internal class MediaBackupPlanner(
    private val scanner: MediaVersionScanner,
    val queue: UploadQueue,
    private val maxQueueSize: Int,
) {
    init {
        require(maxQueueSize in MIN_QUEUE_SIZE..MAX_QUEUE_SIZE) {
            "maxQueueSize must be within $MIN_QUEUE_SIZE..$MAX_QUEUE_SIZE, but was $maxQueueSize"
        }
    }

    /**
     * Enqueues each scanned candidate whose version identity is not already tracked, while the
     * queue holds fewer than `maxQueueSize` entries. The scan runs once and the tracked identities
     * are snapshotted before enqueuing, so two candidates carrying the same identity produce
     * exactly one entry and a re-run over an unchanged media store enqueues nothing.
     *
     * Once the queue reaches the limit, no further candidate is enqueued, every accepted entry
     * stays untouched, and each remaining distinct candidate is counted in
     * [PlanResult.rejectedByCapacity] (`CFG-AND-002 / 2`).
     */
    fun plan(): PlanResult {
        val candidates = scanner.scan()
        val trackedEntries = queue.entries()
        val trackedIdentities = trackedEntries.map { entry -> entry.identity }.toSet()

        val newVersions =
            candidates
                .filter { candidate -> candidate.identity !in trackedIdentities }
                .distinctBy { candidate -> candidate.identity }

        val availableSlots = maxQueueSize - trackedEntries.size
        val acceptedVersions =
            if (availableSlots > 0) newVersions.take(availableSlots) else emptyList()

        acceptedVersions.forEach { candidate ->
            queue.enqueue(
                uploadId = newVersionUploadId(),
                sourceId = candidate.item.id,
                identity = candidate.identity,
                sizeBytes = candidate.item.sizeBytes,
            )
        }

        return PlanResult(
            discoveredCandidates = candidates.size,
            newlyEnqueued = acceptedVersions.size,
            rejectedByCapacity = newVersions.size - acceptedVersions.size,
        )
    }

    private companion object {
        const val MIN_QUEUE_SIZE = 1
        const val MAX_QUEUE_SIZE = 1_000_000
    }
}
