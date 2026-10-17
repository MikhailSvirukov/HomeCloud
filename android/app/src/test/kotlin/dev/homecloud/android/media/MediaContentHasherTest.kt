package dev.homecloud.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.io.ByteArrayInputStream

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
    fun `sha256Hex leaves the caller stream open`() {
        val stream = CloseTrackingInputStream(ABC_BYTES)

        val digest = MediaContentHasher.sha256Hex(stream)

        assertFalse(stream.isClosed)
        assertEquals(ABC_SHA256, digest)
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
    }
}
