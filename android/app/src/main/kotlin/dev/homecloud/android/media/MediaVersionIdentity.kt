package dev.homecloud.android.media

import java.util.UUID

/**
 * The identity of one media file version: the SHA-256 of the original bytes, the original file name
 * and the tracked capture/creation time.
 *
 * Only these three properties participate in version identity; any other media property is
 * ignored.
 */
internal data class MediaVersionIdentity(
    val contentSha256: String,
    val originalName: String,
    val trackedTimeMillis: Long,
)

/**
 * Generates the upload id for a version entering the queue.
 *
 * The shared SRS defines the id as a random UUIDv4 transmitted as 16 bytes (`INT-NET-011`). The
 * queue keeps its canonical text form until the protocol encoder maps it to those bytes. The id is
 * generated once when a version first enters the queue and reused for every retry of that version
 * (`FR-AND-017`), while a new version receives a new random id (`FR-AND-018`).
 */
internal fun newVersionUploadId(): String = UUID.randomUUID().toString()
