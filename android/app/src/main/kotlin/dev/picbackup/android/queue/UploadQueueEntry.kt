package dev.picbackup.android.queue

/**
 * Immutable snapshot of one queued file version.
 *
 * The queue stores entries by value and never mutates an existing instance: state transitions
 * produce a new [UploadQueueEntry] with the same identity but a different [state] and, for
 * failures, an incremented [attemptCount].
 *
 * @property uploadId opaque caller-supplied stable identifier for exactly one file version. The
 *   same id is reused across retries and storage-node switches so that a repeated upload is
 *   idempotent. This model neither generates nor parses the id; its wire format is owned by the
 *   protocol contract.
 * @property sourceId identifier of the local media item the version was derived from.
 * @property sizeBytes size of the payload for this version in bytes.
 * @property dateModifiedEpochSeconds modification timestamp of the local source, in seconds since
 *   the Unix epoch.
 * @property attemptCount number of upload attempts that have failed so far. Starts at zero.
 * @property state current client-side lifecycle state of the version.
 */
data class UploadQueueEntry(
    val uploadId: String,
    val sourceId: Long,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val attemptCount: Int,
    val state: UploadQueueState,
)
