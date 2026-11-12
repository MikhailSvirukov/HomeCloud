package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaContentSource
import dev.homecloud.android.payload.CompressedPayload
import dev.homecloud.android.payload.MediaCompressor
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.upload.UploadMetadata
import dev.homecloud.android.upload.UploadMetadataCodec
import dev.homecloud.android.upload.UploadMetadataLimits
import java.io.IOException
import java.io.InputStream
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.util.UUID

/** Reports that a queued version could not be prepared for upload. */
internal class UploadPreparationException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/** One queued version prepared for the upload arguments and payload. */
internal data class PreparedUpload(
    val uploadId: ByteArray,
    val metadata: UploadMetadata,
    val payload: CompressedPayload,
)

/**
 * Turns one queued version into the upload metadata and the compressed payload.
 *
 * The original bytes are read once, so the SHA-256 and the original size come from the same read
 * that feeds Zstandard compression (`FR-UP-001`, `FR-UP-002`). The complete compressed size is
 * fixed before the returned payload leaves this class, so the first upload frame can be built
 * before any compressed byte is transferred. A source above the configured maximum original size
 * is rejected while it is read (`FR-UP-019`). The queued canonical UUID text maps to the 16-byte
 * upload identifier (`INT-NET-011`).
 *
 * Preparation neither uploads nor changes the queue or any persisted state.
 */
internal class PendingUploadPreparer(
    private val contentSource: MediaContentSource,
    private val compressor: MediaCompressor,
    private val limits: UploadMetadataLimits,
) {
    /**
     * Reads the original bytes at [uri], verifies them against the tracked identity and size,
     * compresses them and builds the upload metadata.
     */
    fun prepare(
        entry: UploadQueueEntry,
        uri: String,
    ): PreparedUpload {
        val uploadId = uploadIdBytes(entry.uploadId)
        return try {
            contentSource.open(uri).use { source ->
                val tracked = TrackedSourceStream(source, limits.maxOriginalSizeBytes)
                val payload = compressor.compress(tracked)
                val measuredSha256 = tracked.measuredDigest()
                requireUnchanged(entry, tracked.measuredSizeBytes, measuredSha256)
                val metadata =
                    UploadMetadata(
                        originalName = entry.identity.originalName,
                        timestampMillis = entry.identity.trackedTimeMillis,
                        originalSizeBytes = tracked.measuredSizeBytes,
                        compressedSizeBytes = payload.totalBytes,
                        sha256 = measuredSha256,
                    )
                validateMetadata(metadata)
                PreparedUpload(uploadId = uploadId, metadata = metadata, payload = payload)
            }
        } catch (exception: UploadPreparationException) {
            throw exception
        } catch (exception: IOException) {
            throw UploadPreparationException(READ_FAILED_MESSAGE, exception)
        } catch (exception: Exception) {
            throw UploadPreparationException(PREPARATION_FAILED_MESSAGE, exception)
        }
    }

    private fun requireUnchanged(
        entry: UploadQueueEntry,
        measuredSizeBytes: Long,
        measuredSha256: ByteArray,
    ) {
        if (measuredSizeBytes != entry.sizeBytes) {
            throw UploadPreparationException(SIZE_CHANGED_MESSAGE)
        }
        if (!measuredSha256.toLowercaseHex().equals(entry.identity.contentSha256, ignoreCase = true)) {
            throw UploadPreparationException(CONTENT_CHANGED_MESSAGE)
        }
    }

    private fun validateMetadata(metadata: UploadMetadata) {
        try {
            UploadMetadataCodec.encode(metadata, limits)
        } catch (exception: IllegalArgumentException) {
            throw UploadPreparationException(METADATA_REJECTED_MESSAGE, exception)
        }
    }
}

private fun uploadIdBytes(text: String): ByteArray {
    val uuid =
        try {
            UUID.fromString(text)
        } catch (exception: IllegalArgumentException) {
            throw UploadPreparationException(INVALID_UPLOAD_ID_MESSAGE, exception)
        }
    if (uuid.toString() != text) {
        throw UploadPreparationException(INVALID_UPLOAD_ID_MESSAGE)
    }
    return ByteBuffer
        .allocate(UPLOAD_ID_BYTES)
        .putLong(uuid.mostSignificantBits)
        .putLong(uuid.leastSignificantBits)
        .array()
}

private class TrackedSourceStream(
    private val source: InputStream,
    private val maxSizeBytes: Long,
) : InputStream() {
    private val digest = MessageDigest.getInstance(DIGEST_ALGORITHM)

    var measuredSizeBytes: Long = 0L
        private set

    override fun read(): Int {
        val value = source.read()
        if (value == END_OF_STREAM) {
            return END_OF_STREAM
        }
        record(1)
        digest.update(value.toByte())
        return value
    }

    override fun read(
        buffer: ByteArray,
        offset: Int,
        length: Int,
    ): Int {
        if (length == 0) {
            return 0
        }
        val read = source.read(buffer, offset, boundedReadLength(length))
        if (read == END_OF_STREAM) {
            return END_OF_STREAM
        }
        record(read)
        digest.update(buffer, offset, read)
        return read
    }

    fun measuredDigest(): ByteArray = digest.digest()

    private fun boundedReadLength(length: Int): Int {
        val remainingBytes = maxSizeBytes - measuredSizeBytes
        return if (remainingBytes >= length.toLong()) {
            length
        } else {
            (remainingBytes + 1L).toInt()
        }
    }

    private fun record(read: Int) {
        measuredSizeBytes += read
        if (measuredSizeBytes > maxSizeBytes) {
            throw UploadPreparationException(SOURCE_TOO_LARGE_MESSAGE)
        }
    }
}

private fun ByteArray.toLowercaseHex(): String {
    val hex = StringBuilder(size * HEX_CHARS_PER_BYTE)
    for (byte in this) {
        val value = byte.toInt() and BYTE_MASK
        hex.append(HEX_DIGITS[value ushr NIBBLE_BITS])
        hex.append(HEX_DIGITS[value and NIBBLE_MASK])
    }
    return hex.toString()
}

private const val DIGEST_ALGORITHM = "SHA-256"
private const val UPLOAD_ID_BYTES = 16
private const val END_OF_STREAM = -1
private const val HEX_CHARS_PER_BYTE = 2
private const val BYTE_MASK = 0xFF
private const val NIBBLE_BITS = 4
private const val NIBBLE_MASK = 0x0F
private val HEX_DIGITS = "0123456789abcdef".toCharArray()

private const val INVALID_UPLOAD_ID_MESSAGE = "the queued upload id is not a canonical UUID"
private const val SOURCE_TOO_LARGE_MESSAGE = "the original file exceeds the configured size limit"
private const val SIZE_CHANGED_MESSAGE = "the queued version size no longer matches the source"
private const val CONTENT_CHANGED_MESSAGE = "the queued version content no longer matches the source"
private const val METADATA_REJECTED_MESSAGE = "the upload metadata is not acceptable"
private const val READ_FAILED_MESSAGE = "the original media could not be read"
private const val PREPARATION_FAILED_MESSAGE = "the queued version could not be prepared"
