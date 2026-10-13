package dev.picbackup.android.queue

/**
 * Client-side model of the pending upload work for the v1 backup flow.
 *
 * An implementation tracks at most one [UploadQueueEntry] per `uploadId`. Enqueueing is
 * idempotent: the same `uploadId` always maps to a single entry so that a retry of a confirmed or
 * unconfirmed upload never creates a duplicate version.
 *
 * The queue is a pure domain model. It performs no I/O, does not persist across process restarts,
 * and does not schedule work; persistence, background scheduling and networking are introduced by
 * later issues.
 *
 * There is deliberately no delete or remove operation: per the v1 specification, a local file
 * deletion must not propagate to the server, so the model exposes no delete command at all.
 */
interface UploadQueue {

    /**
     * Records a new file version as pending upload, or returns the already tracked entry.
     *
     * If [uploadId] is unknown, a new entry is created in [UploadQueueState.PENDING] with an
     * [UploadQueueEntry.attemptCount] of zero. If [uploadId] is already tracked, the existing
     * entry is returned unchanged; no duplicate is created and no attempt or state is reset.
     *
     * @param uploadId opaque stable identifier for this file version.
     * @param sourceId identifier of the local media item.
     * @param sizeBytes payload size in bytes.
     * @param dateModifiedEpochSeconds source modification time in seconds since the Unix epoch.
     * @return the tracked entry, newly created or already existing.
     */
    fun enqueue(
        uploadId: String,
        sourceId: Long,
        sizeBytes: Long,
        dateModifiedEpochSeconds: Long,
    ): UploadQueueEntry

    /**
     * Returns an immutable snapshot of every tracked entry in insertion order.
     *
     * @return the tracked entries; empty when nothing has been enqueued.
     */
    fun entries(): List<UploadQueueEntry>

    /**
     * Returns the oldest entry still in [UploadQueueState.PENDING], or `null` when none is pending.
     *
     * @return the next entry to upload, or `null` when the queue has no pending work.
     */
    fun nextPending(): UploadQueueEntry?

    /**
     * Marks an entry as [UploadQueueState.IN_FLIGHT] so it is no longer returned by [nextPending].
     *
     * @param uploadId identifier of the entry to update.
     * @throws IllegalArgumentException if [uploadId] is not tracked.
     */
    fun markInFlight(uploadId: String)

    /**
     * Marks an entry as [UploadQueueState.COMPLETED].
     *
     * @param uploadId identifier of the entry to update.
     * @throws IllegalArgumentException if [uploadId] is not tracked.
     */
    fun markCompleted(uploadId: String)

    /**
     * Records a failed attempt for an entry.
     *
     * The attempt count is incremented. While the count stays below the configured maximum the
     * entry returns to [UploadQueueState.PENDING]; once the maximum is reached the entry becomes
     * [UploadQueueState.FAILED] and is no longer returned by [nextPending].
     *
     * @param uploadId identifier of the entry to update.
     * @throws IllegalArgumentException if [uploadId] is not tracked.
     */
    fun markFailed(uploadId: String)

    /**
     * Counts the entries currently in [UploadQueueState.PENDING].
     *
     * @return the number of pending entries.
     */
    fun pendingCount(): Int
}
