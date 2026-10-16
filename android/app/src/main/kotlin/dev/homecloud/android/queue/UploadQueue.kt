package dev.homecloud.android.queue

/**
 * Client-side model of the pending upload work for the v1 backup flow.
 *
 * At most one [UploadQueueEntry] is tracked per `uploadId`, and enqueueing is idempotent so a
 * repeated upload id never creates a second version. The queue is a pure domain model: it performs
 * no I/O, persistence, or scheduling.
 *
 * There is deliberately no delete operation, because a local deletion must not propagate to the
 * server.
 */
interface UploadQueue {

    /**
     * Records a file version as pending, or returns the existing entry for a repeated [uploadId]
     * unchanged.
     */
    fun enqueue(
        uploadId: String,
        sourceId: Long,
        sizeBytes: Long,
        dateModifiedEpochSeconds: Long,
    ): UploadQueueEntry

    /**
     * Replaces the tracked entries with the given persisted snapshot so a queue can continue after a
     * restart. Each entry keeps its own state and attempt count; a later entry with an already
     * present upload id replaces the earlier one.
     */
    fun restore(entries: List<UploadQueueEntry>)

    /** Returns an immutable snapshot of every tracked entry in insertion order. */
    fun entries(): List<UploadQueueEntry>

    /** Returns the oldest pending entry, or `null` when nothing is pending. */
    fun nextPending(): UploadQueueEntry?

    /** Marks an entry [UploadQueueState.IN_FLIGHT] so [nextPending] skips it. */
    fun markInFlight(uploadId: String)

    /** Marks an entry [UploadQueueState.COMPLETED]. */
    fun markCompleted(uploadId: String)

    /**
     * Records a failed attempt: the entry stays [UploadQueueState.PENDING] until the attempt budget
     * is exhausted, then becomes [UploadQueueState.FAILED].
     */
    fun markFailed(uploadId: String)

    /**
     * Starts a new retry cycle: every [UploadQueueState.FAILED] entry returns to
     * [UploadQueueState.PENDING] with its attempt budget reset, so work whose nodes report no quorum
     * stays queued. Other states are untouched.
     */
    fun requeueFailed(): Int

    /** Counts the entries currently pending. */
    fun pendingCount(): Int
}
