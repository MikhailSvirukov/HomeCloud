package dev.homecloud.android.payload

import com.github.luben.zstd.Zstd
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.util.Random

class MediaCompressorTest {
    @Test
    fun `small input round trips through the concatenated frame`() {
        val original = SMALL_INPUT

        val payload = MediaCompressor().compress(ByteArrayInputStream(original))

        assertArrayEquals(original, decompress(payload, original.size))
    }

    @Test
    fun `totalBytes and blockCount describe the whole payload`() {
        val original = pseudoRandomBytes(BLOCK_INPUT_BYTES)

        val payload =
            MediaCompressor(blockSizeBytes = SMALL_BLOCK_SIZE_BYTES)
                .compress(ByteArrayInputStream(original))

        assertTrue(payload.blockCount > 1)
        assertEquals(payload.blocks.sumOf { it.size.toLong() }, payload.totalBytes)
        assertEquals(frameOf(payload).size.toLong(), payload.totalBytes)
        assertEquals(payload.blocks.size, payload.blockCount)
    }

    @Test
    fun `blocks are non-empty and bounded with a shorter final block`() {
        val original = pseudoRandomBytes(BLOCK_INPUT_BYTES)

        val payload =
            MediaCompressor(blockSizeBytes = SMALL_BLOCK_SIZE_BYTES)
                .compress(ByteArrayInputStream(original))

        assertTrue(payload.blockCount > 1)
        payload.blocks.forEach { block ->
            assertTrue(block.isNotEmpty())
            assertTrue(block.size <= SMALL_BLOCK_SIZE_BYTES)
        }
        assertTrue(payload.blocks.last().size < SMALL_BLOCK_SIZE_BYTES)
        assertArrayEquals(original, decompress(payload, original.size))
    }

    @Test
    fun `empty input still produces a decodable frame`() {
        val payload = MediaCompressor().compress(ByteArrayInputStream(EMPTY_INPUT))

        val frame = frameOf(payload)

        assertTrue(frame.isNotEmpty())
        assertArrayEquals(EMPTY_INPUT, Zstd.decompress(frame, EMPTY_INPUT.size))
    }

    @Test
    fun `default minimum and maximum levels produce a decodable frame`() {
        val original = LEVEL_INPUT
        val compressors =
            listOf(
                MediaCompressor(),
                MediaCompressor(compressionLevel = MediaCompressor.MIN_LEVEL),
                MediaCompressor(compressionLevel = MediaCompressor.MAX_LEVEL),
            )

        compressors.forEach { compressor ->
            val payload = compressor.compress(ByteArrayInputStream(original))
            assertArrayEquals(original, decompress(payload, original.size))
        }
    }

    @Test
    fun `compression level outside the supported range is rejected`() {
        listOf(MediaCompressor.MIN_LEVEL - 1, MediaCompressor.MAX_LEVEL + 1).forEach { level ->
            assertThrows(IllegalArgumentException::class.java) {
                MediaCompressor(compressionLevel = level)
            }
        }
    }

    @Test
    fun `non-positive block size is rejected`() {
        listOf(0, -1).forEach { blockSizeBytes ->
            assertThrows(IllegalArgumentException::class.java) {
                MediaCompressor(blockSizeBytes = blockSizeBytes)
            }
        }
    }

    @Test
    fun `compress leaves the caller input stream open`() {
        val original = SMALL_INPUT
        val stream = CloseTrackingInputStream(original)

        val payload = MediaCompressor().compress(stream)

        assertFalse(stream.isClosed)
        assertArrayEquals(original, decompress(payload, original.size))
    }

    private fun decompress(
        payload: CompressedPayload,
        originalSize: Int,
    ): ByteArray = Zstd.decompress(frameOf(payload), originalSize)

    private fun frameOf(payload: CompressedPayload): ByteArray {
        val frame = ByteArrayOutputStream()
        payload.blocks.forEach { block -> frame.write(block) }
        return frame.toByteArray()
    }

    private fun pseudoRandomBytes(size: Int): ByteArray {
        val bytes = ByteArray(size)
        Random(RANDOM_SEED).nextBytes(bytes)
        return bytes
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

    private companion object {
        val SMALL_INPUT = "homecloud zstandard payload".toByteArray(Charsets.UTF_8)
        val EMPTY_INPUT = ByteArray(0)
        val LEVEL_INPUT = "homecloud compression level ".repeat(32).toByteArray(Charsets.UTF_8)
        const val BLOCK_INPUT_BYTES = 10_000
        const val SMALL_BLOCK_SIZE_BYTES = 1024
        const val RANDOM_SEED = 42L
    }
}
