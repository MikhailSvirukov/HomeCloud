package dev.homecloud.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.InputStream

class MediaContentHasherTest {
    @Test
    fun `sha256Hex of abc matches the published SHA-256 vector`() {
        val digest = MediaContentHasher.sha256Hex(ByteArrayInputStream(ABC_BYTES))

        assertEquals(ABC_SHA256, digest)
    }

    @Test
    fun `sha256Hex of an empty stream matches the published SHA-256 vector`() {
        val digest = MediaContentHasher.sha256Hex(ByteArrayInputStream(EMPTY_BYTES))

        assertEquals(EMPTY_SHA256, digest)
    }

    @Test
    fun `sha256Hex streams across multiple reads and matches the published vector`() {
        val stream = OneBytePerReadInputStream(ABC_BYTES)

        val digest = MediaContentHasher.sha256Hex(stream)

        assertEquals(ABC_SHA256, digest)
        assertTrue(stream.readCalls > 1)
    }

    @Test
    fun `sha256Hex leaves the caller stream open`() {
        val stream = CloseTrackingInputStream(ABC_BYTES)

        val digest = MediaContentHasher.sha256Hex(stream)

        assertFalse(stream.isClosed)
        assertEquals(ABC_SHA256, digest)
    }

    private class OneBytePerReadInputStream(
        private val bytes: ByteArray,
    ) : InputStream() {
        var readCalls = 0
            private set

        private var position = 0

        override fun read(): Int {
            readCalls++
            return if (position < bytes.size) {
                bytes[position++].toInt() and BYTE_MASK
            } else {
                END_OF_STREAM
            }
        }

        override fun read(
            buffer: ByteArray,
            offset: Int,
            length: Int,
        ): Int {
            readCalls++
            if (length == 0) {
                return 0
            }
            if (position >= bytes.size) {
                return END_OF_STREAM
            }
            buffer[offset] = bytes[position++]
            return 1
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

    private companion object {
        val ABC_BYTES = "abc".toByteArray(Charsets.UTF_8)
        val EMPTY_BYTES = ByteArray(0)
        const val ABC_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        const val EMPTY_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        const val BYTE_MASK = 0xFF
        const val END_OF_STREAM = -1
    }
}
