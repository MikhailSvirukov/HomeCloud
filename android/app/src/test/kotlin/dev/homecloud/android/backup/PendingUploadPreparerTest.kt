package dev.homecloud.android.backup

import com.github.luben.zstd.ZstdInputStream
import dev.homecloud.android.media.MediaContentSource
import dev.homecloud.android.media.MediaVersionIdentity
import dev.homecloud.android.payload.MediaCompressor
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.testsupport.Hex
import dev.homecloud.android.upload.UploadMetadataLimits
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.security.MessageDigest
import java.util.Locale

class PendingUploadPreparerTest {
    @Test
    fun `prepare builds metadata from the queued version and the measured content`() {
        val prepared = preparer().prepare(entry(), URI)

        assertEquals(ORIGINAL_NAME, prepared.metadata.originalName)
        assertEquals(TRACKED_TIME_MILLIS, prepared.metadata.timestampMillis)
        assertEquals(ORIGINAL_BYTES.size.toLong(), prepared.metadata.originalSizeBytes)
        assertEquals(prepared.payload.totalBytes, prepared.metadata.compressedSizeBytes)
        assertEquals(SHA256_BYTES, prepared.metadata.sha256.size)
        assertArrayEquals(Hex.decode(SHA256_HEX), prepared.metadata.sha256)
    }

    @Test
    fun `prepare produces a payload that decompresses to the original bytes`() {
        val prepared = preparer().prepare(entry(), URI)

        assertArrayEquals(ORIGINAL_BYTES, decompress(prepared))
    }

    @Test
    fun `prepare maps the queued uuid text to its sixteen bytes`() {
        val prepared = preparer().prepare(entry(uploadId = UPLOAD_ID), URI)

        assertArrayEquals(Hex.decode(UPLOAD_ID_HEX), prepared.uploadId)
    }

    @Test
    fun `prepare accepts an uppercase tracked digest`() {
        val identity = identity(contentSha256 = SHA256_HEX.uppercase(Locale.ROOT))

        val prepared = preparer().prepare(entry(identity = identity), URI)

        assertArrayEquals(Hex.decode(SHA256_HEX), prepared.metadata.sha256)
    }

    @Test
    fun `prepare rejects a version whose content changed after it was queued`() {
        val identity = identity(contentSha256 = OTHER_SHA256_HEX)

        assertThrows(UploadPreparationException::class.java) {
            preparer().prepare(entry(identity = identity), URI)
        }
    }

    @Test
    fun `prepare rejects a version whose measured size differs from the queued size`() {
        val entry = entry(sizeBytes = ORIGINAL_BYTES.size + 1L)

        assertThrows(UploadPreparationException::class.java) {
            preparer().prepare(entry, URI)
        }
    }

    @Test
    fun `prepare rejects a non-canonical upload id`() {
        assertThrows(UploadPreparationException::class.java) {
            preparer().prepare(entry(uploadId = INVALID_UPLOAD_ID), URI)
        }
    }

    @Test
    fun `prepare reports a source read failure as an upload preparation failure`() {
        val failure = IOException(READ_FAILURE_MESSAGE)
        val source = MediaContentSource { FailingInputStream(failure) }

        val exception =
            assertThrows(UploadPreparationException::class.java) {
                preparer(contentSource = source).prepare(entry(), URI)
            }

        assertSame(failure, exception.cause)
    }

    @Test
    fun `prepare rejects a source above the configured limit without reading it to the end`() {
        val source = CountingInputStream(ByteArray(OVERSIZED_BYTES))
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = LIMIT_BYTES)

        assertThrows(UploadPreparationException::class.java) {
            preparer(contentSource = MediaContentSource { source }, limits = limits)
                .prepare(entry(sizeBytes = OVERSIZED_BYTES.toLong()), URI)
        }

        assertFalse(source.isExhausted)
        assertTrue(source.readBytes > 0L)
        assertTrue(source.readBytes <= LIMIT_BYTES + 1L)
    }

    @Test
    fun `prepare rejects a metadata name above the configured limit`() {
        val limits =
            UploadMetadataLimits(
                maxOriginalSizeBytes = ORIGINAL_BYTES.size.toLong(),
                maxNameBytes = NAME_LIMIT_BYTES,
            )
        val identity = identity(originalName = "n".repeat(NAME_LIMIT_BYTES + 1))

        assertThrows(UploadPreparationException::class.java) {
            preparer(limits = limits).prepare(entry(identity = identity), URI)
        }
    }

    @Test
    fun `prepare closes the source stream on success`() {
        val source = CloseTrackingInputStream(ORIGINAL_BYTES)

        preparer(contentSource = MediaContentSource { source }).prepare(entry(), URI)

        assertTrue(source.isClosed)
    }

    @Test
    fun `prepare closes the source stream when the queued version no longer matches`() {
        val source = CloseTrackingInputStream(ORIGINAL_BYTES)
        val identity = identity(contentSha256 = OTHER_SHA256_HEX)

        assertThrows(UploadPreparationException::class.java) {
            preparer(contentSource = MediaContentSource { source }).prepare(entry(identity = identity), URI)
        }

        assertTrue(source.isClosed)
    }

    private fun preparer(
        contentSource: MediaContentSource = MediaContentSource { ByteArrayInputStream(ORIGINAL_BYTES) },
        limits: UploadMetadataLimits = UploadMetadataLimits(maxOriginalSizeBytes = ORIGINAL_BYTES.size.toLong()),
    ): PendingUploadPreparer =
        PendingUploadPreparer(
            contentSource = contentSource,
            compressor = MediaCompressor(blockSizeBytes = BLOCK_SIZE_BYTES),
            limits = limits,
        )

    private fun identity(
        contentSha256: String = SHA256_HEX,
        originalName: String = ORIGINAL_NAME,
        trackedTimeMillis: Long = TRACKED_TIME_MILLIS,
    ): MediaVersionIdentity =
        MediaVersionIdentity(
            contentSha256 = contentSha256,
            originalName = originalName,
            trackedTimeMillis = trackedTimeMillis,
        )

    private fun entry(
        uploadId: String = UPLOAD_ID,
        identity: MediaVersionIdentity = identity(),
        sizeBytes: Long = ORIGINAL_BYTES.size.toLong(),
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = SOURCE_ID,
            identity = identity,
            sizeBytes = sizeBytes,
            attemptCount = 0,
            state = UploadQueueState.PENDING,
        )

    private fun decompress(prepared: PreparedUpload): ByteArray {
        val frame = ByteArrayOutputStream()
        prepared.payload.blocks.forEach { block -> frame.write(block) }
        return ZstdInputStream(ByteArrayInputStream(frame.toByteArray())).use { stream -> stream.readBytes() }
    }

    private class CountingInputStream(
        private val bytes: ByteArray,
    ) : InputStream() {
        private var position = 0

        var readBytes: Long = 0L
            private set

        val isExhausted: Boolean
            get() = position >= bytes.size

        override fun read(): Int {
            if (position >= bytes.size) {
                return END_OF_STREAM
            }
            readBytes += 1
            return bytes[position++].toInt() and BYTE_MASK
        }

        override fun read(
            buffer: ByteArray,
            offset: Int,
            length: Int,
        ): Int {
            if (length == 0) {
                return 0
            }
            if (position >= bytes.size) {
                return END_OF_STREAM
            }
            val count = minOf(length, bytes.size - position)
            bytes.copyInto(buffer, offset, position, position + count)
            position += count
            readBytes += count
            return count
        }
    }

    private class CloseTrackingInputStream(
        bytes: ByteArray,
    ) : ByteArrayInputStream(bytes) {
        var isClosed = false
            private set

        override fun close() {
            isClosed = true
            super.close()
        }
    }

    private class FailingInputStream(
        private val failure: IOException,
    ) : InputStream() {
        override fun read(): Int = throw failure

        override fun read(
            buffer: ByteArray,
            offset: Int,
            length: Int,
        ): Int = throw failure
    }

    private companion object {
        const val URI = "content://media/external/file/1"
        const val SOURCE_ID = 1L
        const val UPLOAD_ID = "123e4567-e89b-42d3-a456-426614174000"
        const val UPLOAD_ID_HEX = "123e4567e89b42d3a456426614174000"
        const val INVALID_UPLOAD_ID = "1-1-1-1-1"
        const val ORIGINAL_NAME = "holiday photo.jpg"
        const val TRACKED_TIME_MILLIS = 1_700_000_123_456L
        const val BLOCK_SIZE_BYTES = 16
        const val SHA256_BYTES = 32
        const val OTHER_SHA256_HEX = "0000000000000000000000000000000000000000000000000000000000000000"
        const val NAME_LIMIT_BYTES = 8
        const val LIMIT_BYTES = 1024L
        const val OVERSIZED_BYTES = 4096
        const val SHA256_ALGORITHM = "SHA-256"
        const val BYTE_MASK = 0xFF
        const val HEX_RADIX = 16
        const val HEX_DIGITS_PER_BYTE = 2
        const val END_OF_STREAM = -1
        const val READ_FAILURE_MESSAGE = "the source stream failed"

        val ORIGINAL_BYTES = "homecloud pending upload ".repeat(64).toByteArray(Charsets.UTF_8)
        val SHA256_HEX =
            MessageDigest
                .getInstance(SHA256_ALGORITHM)
                .digest(ORIGINAL_BYTES)
                .joinToString(separator = "") { byte ->
                    (byte.toInt() and BYTE_MASK).toString(HEX_RADIX).padStart(HEX_DIGITS_PER_BYTE, '0')
                }
    }
}
