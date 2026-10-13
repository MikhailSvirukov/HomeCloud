package dev.picbackup.android.queue

/**
 * In-memory implementation of [UploadQueue] backed by a single insertion-ordered map.
 *
 * Entries are keyed by `uploadId`, so [enqueue] is idempotent: a repeated id returns the existing
 * entry instead of adding a second one. Iteration order is insertion order, which makes
 * [nextPending] deterministic.
 *
 * This class is intentionally not thread-safe. All access must happen on a single thread or be
 * externally synchronized; concurrency is out of scope for this domain model and is handled by the
 * scheduling layer in a later issue. No locking is performed here.
 *
 * There is no delete or remove operation, by design: the v1 specification forbids propagating a
 * local file deletion to the server, so the model exposes no delete command at all.
 *
 * @param maxAttempts number of failed attempts after which an entry becomes
 *   [UploadQueueState.FAILED] instead of returning to [UploadQueueState.PENDING]. Must be positive.
 * @throws IllegalArgumentException if [maxAttempts] is not positive.
 */
class InMemoryUploadQueue(private val maxAttempts: Int) : UploadQueue {

    init {
        require(maxAttempts > 0) { "maxAttempts must be positive but was $maxAttempts" }
    }

    private val entriesById = LinkedHashMap<String, UploadQueueEntry>()

    override fun enqueue(
        uploadId: String,
        sourceId: Long,
        sizeBytes: Long,
        dateModifiedEpochSeconds: Long,
    ): UploadQueueEntry =
        entriesById.getOrPut(uploadId) {
            UploadQueueEntry(
                uploadId = uploadId,
                sourceId = sourceId,
                sizeBytes = sizeBytes,
                dateModifiedEpochSeconds = dateModifiedEpochSeconds,
                attemptCount = 0,
                state = UploadQueueState.PENDING,
            )
        }

    override fun entries(): List<UploadQueueEntry> = entriesById.values.toList()

    override fun nextPending(): UploadQueueEntry? =
        entriesById.values.firstOrNull { it.state == UploadQueueState.PENDING }

    override fun markInFlight(uploadId: String) {
        update(uploadId) { entry -> entry.copy(state = UploadQueueState.IN_FLIGHT) }
    }

    override fun markCompleted(uploadId: String) {
        update(uploadId) { entry -> entry.copy(state = UploadQueueState.COMPLETED) }
    }

    override fun markFailed(uploadId: String) {
        update(uploadId) { entry ->
            val attemptCount = entry.attemptCount + 1
            val state =
                if (attemptCount >= maxAttempts) {
                    UploadQueueState.FAILED
                } else {
                    UploadQueueState.PENDING
                }
            entry.copy(attemptCount = attemptCount, state = state)
        }
    }

    override fun pendingCount(): Int =
        entriesById.values.count { it.state == UploadQueueState.PENDING }

    private fun update(uploadId: String, transform: (UploadQueueEntry) -> UploadQueueEntry) {
        val existing =
            entriesById[uploadId]
                ?: throw IllegalArgumentException(UNKNOWN_UPLOAD_ID_PREFIX + uploadId)
        entriesById[uploadId] = transform(existing)
    }

    private companion object {
        const val UNKNOWN_UPLOAD_ID_PREFIX = "Unknown uploadId: "
    }
}
