package dev.homecloud.android.status

import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState

/** Phase of the backup cycle the main screen reports (`FR-AND-044 / 1`). */
internal enum class CycleState {
    /** No cycle is running and no last result is known yet. */
    IDLE,

    /** A cycle is discovering media. */
    SCANNING,

    /** A cycle is uploading queued versions. */
    UPLOADING,

    /** The last cycle finished successfully. */
    COMPLETED,

    /** The last cycle ended with an error. */
    FAILED,
}

/** Result of one completed backup cycle (`FR-AND-045 / 1`). */
internal enum class CycleOutcome {
    /** The cycle reached the end of planning and persisted its queue. */
    SUCCESS,

    /** The cycle stopped on an error. */
    FAILURE,
}

/**
 * User-facing class of a failed backup cycle (`FR-AND-047 / 1`).
 *
 * The categories name only the failing area, so the screen never has to show an exception message,
 * a password, a key, or a file name.
 */
internal enum class LastErrorCategory {
    /** The device media library could not be read. */
    MEDIA_LIBRARY_UNREADABLE,

    /** The persisted upload queue could not be read or written. */
    QUEUE_UNAVAILABLE,

    /** The server could not be reached. */
    TRANSPORT_UNAVAILABLE,

    /** Any other failure. */
    INTERNAL,
}

/**
 * Everything the main screen shows about the backup cycle (`FR-AND-044 / 1`, `FR-AND-045 / 1`,
 * `FR-AND-046 / 1`, `FR-AND-047 / 1`).
 *
 * The pending count covers versions still waiting for upload and versions currently uploading;
 * versions the server confirmed or that exhausted their attempts are not pending.
 */
internal data class BackupStatus(
    val state: CycleState,
    val lastCompletedAtMillis: Long?,
    val lastOutcome: CycleOutcome?,
    val pendingVersions: Int,
    val lastError: LastErrorCategory?,
)

/**
 * Builds the screen status from the queue snapshot and the recorded last cycle
 * (`FR-AND-044 / 1`, `FR-AND-045 / 1`, `FR-AND-046 / 1`, `FR-AND-047 / 1`).
 *
 * A missing last cycle leaves the screen idle with no completion time, result, or error. A recorded
 * cycle supplies the time and result, and the caller-supplied state names what the cycle is doing
 * now. A successful record never reports an error.
 */
internal object BackupStatusReader {
    fun statusOf(
        entries: List<UploadQueueEntry>,
        last: LastCycleRecord?,
        state: CycleState,
    ): BackupStatus {
        val pendingVersions =
            entries.count { entry ->
                entry.state == UploadQueueState.PENDING || entry.state == UploadQueueState.IN_FLIGHT
            }
        if (last == null) {
            return BackupStatus(
                state = state,
                lastCompletedAtMillis = null,
                lastOutcome = null,
                pendingVersions = pendingVersions,
                lastError = null,
            )
        }
        return BackupStatus(
            state = state,
            lastCompletedAtMillis = last.completedAtMillis,
            lastOutcome = last.outcome,
            pendingVersions = pendingVersions,
            lastError = if (last.outcome == CycleOutcome.SUCCESS) null else last.error,
        )
    }
}
