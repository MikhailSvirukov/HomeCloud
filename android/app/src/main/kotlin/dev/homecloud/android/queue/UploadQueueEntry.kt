package dev.homecloud.android.queue

/**
 * Immutable snapshot of one queued file version.
 *
 * Transitions produce a new instance with the same identity and a different state rather than
 * mutating an existing entry. The [uploadId] is opaque and reused across retries and storage-node
 * switches; its wire format is owned by the protocol contract, not by this model.
 */
internal data class UploadQueueEntry(
    val uploadId: String,
    val sourceId: Long,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val attemptCount: Int,
    val state: UploadQueueState,
)
