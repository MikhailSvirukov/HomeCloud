package dev.homecloud.android.media

import java.nio.ByteBuffer
import java.nio.ByteOrder

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
 * Derives the opaque upload id identifying one version.
 *
 * The derivation is local and stable: equal identities produce equal ids, while changing the content
 * hash, original name or tracked time produces a different id. Each string is encoded as a
 * four-byte big-endian UTF-8 byte length followed by its bytes, then the tracked time follows as
 * eight big-endian bytes, and SHA-256 hashes the result. The lowercase hex digest is opaque; its
 * wire format belongs to the protocol contract.
 */
internal fun uploadIdOf(identity: MediaVersionIdentity): String {
    val contentHashBytes = identity.contentSha256.encodeToByteArray()
    val originalNameBytes = identity.originalName.encodeToByteArray()
    val capacity =
        STRING_LENGTH_BYTES +
            contentHashBytes.size +
            STRING_LENGTH_BYTES +
            originalNameBytes.size +
            TIMESTAMP_BYTES
    val canonical =
        ByteBuffer
            .allocate(capacity)
            .order(ByteOrder.BIG_ENDIAN)
            .putInt(contentHashBytes.size)
            .put(contentHashBytes)
            .putInt(originalNameBytes.size)
            .put(originalNameBytes)
            .putLong(identity.trackedTimeMillis)
            .array()
    return MediaContentHasher.sha256Hex(canonical.inputStream())
}

private const val STRING_LENGTH_BYTES = Int.SIZE_BYTES
private const val TIMESTAMP_BYTES = Long.SIZE_BYTES
