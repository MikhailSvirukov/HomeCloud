package dev.homecloud.android.queue

/**
 * In-memory [UploadQueue] backed by an insertion-ordered map keyed by `uploadId`, so [enqueue] is
 * idempotent and [nextPending] is deterministic. Not thread-safe by design: access must stay on one
 * thread or be externally synchronized, since concurrency is handled by the scheduling layer. There
 * is no delete operation, by design, because a local deletion must not propagate to the server.
 */
internal class InMemoryUploadQueue(
    private val maxAttempts: Int,
) : UploadQueue {
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

    override fun restore(entries: List<UploadQueueEntry>) {
        entriesById.clear()
        entries.forEach { entry -> entriesById[entry.uploadId] = entry }
    }

    override fun entries(): List<UploadQueueEntry> = entriesById.values.toList()

    override fun nextPending(): UploadQueueEntry? = entriesById.values.firstOrNull { it.state == UploadQueueState.PENDING }

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

    override fun pendingCount(): Int = entriesById.values.count { it.state == UploadQueueState.PENDING }

    override fun requeueFailed(): Int {
        val failedUploadIds =
            entriesById.values
                .filter { it.state == UploadQueueState.FAILED }
                .map { it.uploadId }
        failedUploadIds.forEach { uploadId ->
            update(uploadId) { entry ->
                entry.copy(attemptCount = 0, state = UploadQueueState.PENDING)
            }
        }
        return failedUploadIds.size
    }

    private fun update(
        uploadId: String,
        transform: (UploadQueueEntry) -> UploadQueueEntry,
    ) {
        val existing =
            entriesById[uploadId]
                ?: throw IllegalArgumentException(UNKNOWN_UPLOAD_ID_PREFIX + uploadId)
        entriesById[uploadId] = transform(existing)
    }

    private companion object {
        const val UNKNOWN_UPLOAD_ID_PREFIX = "Unknown uploadId: "
    }
}
