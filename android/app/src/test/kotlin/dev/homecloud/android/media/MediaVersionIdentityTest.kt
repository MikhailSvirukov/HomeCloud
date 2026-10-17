package dev.homecloud.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaVersionIdentityTest {
    @Test
    fun `equal identities produce the same upload id across calls`() {
        val version = identity()

        assertEquals(uploadIdOf(version), uploadIdOf(version.copy()))
    }

    @Test
    fun `changed content hash produces a different upload id`() {
        val changed = identity(contentSha256 = OTHER_CONTENT_HASH)

        assertNotEquals(uploadIdOf(identity()), uploadIdOf(changed))
    }

    @Test
    fun `changed original name produces a different upload id`() {
        val changed = identity(originalName = OTHER_ORIGINAL_NAME)

        assertNotEquals(uploadIdOf(identity()), uploadIdOf(changed))
    }

    @Test
    fun `changed tracked time produces a different upload id`() {
        val changed = identity(trackedTimeMillis = OTHER_TRACKED_TIME_MILLIS)

        assertNotEquals(uploadIdOf(identity()), uploadIdOf(changed))
    }

    @Test
    fun `canonical encoding separates adjacent hash and name boundaries`() {
        val hashThenName = identity(contentSha256 = HASH_AB, originalName = NAME_C)
        val noSeparator = identity(contentSha256 = HASH_A, originalName = NAME_BC)

        assertNotEquals(uploadIdOf(hashThenName), uploadIdOf(noSeparator))
    }

    @Test
    fun `upload id is 64 lowercase hexadecimal characters`() {
        val uploadId = uploadIdOf(identity())

        assertEquals(SHA256_HEX_LENGTH, uploadId.length)
        assertTrue(SHA256_HEX_PATTERN.matches(uploadId))
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
        const val HASH_AB = "ab"
        const val HASH_A = "a"
        const val NAME_C = "c"
        const val NAME_BC = "bc"
        const val SHA256_HEX_LENGTH = 64
        val SHA256_HEX_PATTERN = Regex("[0-9a-f]{64}")
    }
}
