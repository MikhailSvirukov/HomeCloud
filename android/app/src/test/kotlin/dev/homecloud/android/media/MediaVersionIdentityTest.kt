package dev.homecloud.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test
import java.util.UUID

class MediaVersionIdentityTest {
    @Test
    fun `equal field values produce equal identities`() {
        val version = identity()

        assertEquals(version, version.copy())
    }

    @Test
    fun `changed content hash produces a different identity`() {
        val changed = identity(contentSha256 = OTHER_CONTENT_HASH)

        assertNotEquals(identity(), changed)
    }

    @Test
    fun `changed original name produces a different identity`() {
        val changed = identity(originalName = OTHER_ORIGINAL_NAME)

        assertNotEquals(identity(), changed)
    }

    @Test
    fun `changed tracked time produces a different identity`() {
        val changed = identity(trackedTimeMillis = OTHER_TRACKED_TIME_MILLIS)

        assertNotEquals(identity(), changed)
    }

    @Test
    fun `successive upload ids differ`() {
        assertNotEquals(newVersionUploadId(), newVersionUploadId())
    }

    @Test
    fun `new upload id is a canonical UUIDv4 with the RFC 4122 variant`() {
        val uploadId = newVersionUploadId()
        val uuid = UUID.fromString(uploadId)

        assertEquals(uploadId, uuid.toString())
        assertEquals(UUID_VERSION_4, uuid.version())
        assertEquals(UUID_VARIANT_RFC_4122, uuid.variant())
    }

    private fun identity(
        contentSha256: String = CONTENT_HASH,
        originalName: String = ORIGINAL_NAME,
        trackedTimeMillis: Long = TRACKED_TIME_MILLIS,
    ) = MediaVersionIdentity(
        contentSha256 = contentSha256,
        originalName = originalName,
        trackedTimeMillis = trackedTimeMillis,
    )

    private companion object {
        const val CONTENT_HASH = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        const val OTHER_CONTENT_HASH = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        const val ORIGINAL_NAME = "photo.jpg"
        const val OTHER_ORIGINAL_NAME = "renamed-photo.jpg"
        const val TRACKED_TIME_MILLIS = 1_700_000_000_000L
        const val OTHER_TRACKED_TIME_MILLIS = 1_700_000_000_001L
        const val UUID_VERSION_4 = 4
        const val UUID_VARIANT_RFC_4122 = 2
    }
}
