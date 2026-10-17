package dev.homecloud.android.queue

import dev.homecloud.android.media.MediaVersionIdentity

/**
 * Immutable snapshot of one queued file version.
 *
 * Transitions produce a new instance with the same identity and a different state rather than
 * mutating an existing entry. The [identity] is the version discriminator: a changed hash, original
 * name or tracked time is a different version. The [uploadId] is opaque and reused across retries
 * and storage-node switches of this version; its wire format is owned by the protocol contract, not
 * by this model.
 */
internal data class UploadQueueEntry(
    val uploadId: String,
    val sourceId: Long,
    val identity: MediaVersionIdentity,
    val sizeBytes: Long,
    val attemptCount: Int,
    val state: UploadQueueState,
)
