package dev.picbackup.android.queue

/**
 * Lifecycle state of a single [UploadQueueEntry] in the local upload queue.
 *
 * The states describe only the client-side orchestration of one file version. A version is
 * considered done once the server has confirmed it, which the queue models as [COMPLETED].
 */
enum class UploadQueueState {
    /**
     * Waiting to be selected for upload. This is the only state returned by
     * [UploadQueue.nextPending].
     */
    PENDING,

    /**
     * Currently being uploaded. The entry stays in this state until the attempt is reported as
     * [COMPLETED] or [FAILED].
     */
    IN_FLIGHT,

    /**
     * Confirmed by the server. A completed entry is never uploaded again; re-enqueueing the same
     * `uploadId` returns the existing entry unchanged rather than creating a duplicate.
     */
    COMPLETED,

    /**
     * Exhausted the configured attempt budget. A failed entry is not retried automatically and is
     * not returned by [UploadQueue.nextPending].
     */
    FAILED,
}
