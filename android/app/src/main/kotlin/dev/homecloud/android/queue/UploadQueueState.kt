package dev.homecloud.android.queue

/**
 * Client-side orchestration state of one file version in the local upload queue. A version is done
 * once the server has confirmed it, modelled as [COMPLETED].
 */
enum class UploadQueueState {
    /** Waiting for upload; the only state [UploadQueue.nextPending] returns. */
    PENDING,

    /** Being uploaded; the entry leaves this state when its attempt is completed or failed. */
    IN_FLIGHT,

    /** Confirmed by the server and never uploaded again. */
    COMPLETED,

    /** Attempt budget exhausted; not retried automatically and never returned by [nextPending]. */
    FAILED,
}
