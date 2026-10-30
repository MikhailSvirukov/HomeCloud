package dev.homecloud.android.catalog

import dev.homecloud.android.media.MediaVersionIdentity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the catalog comparison of `FR-CAT-003 / 1` and `FR-CAT-004 / 1`: a local version is
 * selected unless the confirmed catalog already contains the same content hash, source name and
 * tracked time.
 */
class CatalogComparisonTest {
    @Test
    fun `a fully matching version is not selected`() {
        val version = version()

        assertTrue(CatalogComparison.missingVersions(listOf(version), listOf(entry())).isEmpty())
    }

    @Test
    fun `a changed content hash selects the version`() {
        val other = version(contentSha256 = "ff".repeat(32))

        assertEquals(listOf(other), CatalogComparison.missingVersions(listOf(other), listOf(entry())))
    }

    @Test
    fun `a changed name selects the version`() {
        val other = version(originalName = "other.jpg")

        assertEquals(listOf(other), CatalogComparison.missingVersions(listOf(other), listOf(entry())))
    }

    @Test
    fun `a changed tracked time selects the version`() {
        val other = version(trackedTimeMillis = TRACKED_TIME_MILLIS + 1)

        assertEquals(listOf(other), CatalogComparison.missingVersions(listOf(other), listOf(entry())))
    }

    @Test
    fun `an empty catalog selects every local version`() {
        val versions = listOf(version(), version(contentSha256 = "ab".repeat(32)))

        assertEquals(versions, CatalogComparison.missingVersions(versions, emptyList()))
    }

    @Test
    fun `an empty local list selects nothing`() {
        assertTrue(CatalogComparison.missingVersions(emptyList(), listOf(entry())).isEmpty())
    }

    @Test
    fun `the input order is preserved`() {
        val first = version(contentSha256 = "01".repeat(32))
        val second = version(contentSha256 = "02".repeat(32))
        val third = version(contentSha256 = "03".repeat(32))

        assertEquals(
            listOf(first, second, third),
            CatalogComparison.missingVersions(listOf(third, first, second), emptyList()),
        )
    }

    @Test
    fun `a repeated identity appears once`() {
        val duplicate = version()

        assertEquals(listOf(duplicate), CatalogComparison.missingVersions(listOf(duplicate, duplicate), emptyList()))
    }

    @Test
    fun `differing catalog sizes still match`() {
        val entry = entry(originalSizeBytes = ORIGINAL_SIZE_BYTES + 1, compressedSizeBytes = COMPRESSED_SIZE_BYTES + 1)

        assertTrue(CatalogComparison.missingVersions(listOf(version()), listOf(entry)).isEmpty())
    }

    @Test
    fun `a digest of a different length never matches`() {
        val entry = entry(sha256 = ByteArray(SHA256_BYTES - 1) { index -> (index + 1).toByte() })

        assertEquals(listOf(version()), CatalogComparison.missingVersions(listOf(version()), listOf(entry)))
    }

    private fun version(
        contentSha256: String = CONTENT_SHA256_HEX,
        originalName: String = ORIGINAL_NAME,
        trackedTimeMillis: Long = TRACKED_TIME_MILLIS,
    ): MediaVersionIdentity =
        MediaVersionIdentity(
            contentSha256 = contentSha256,
            originalName = originalName,
            trackedTimeMillis = trackedTimeMillis,
        )

    private fun entry(
        sha256: ByteArray = hexToBytes(CONTENT_SHA256_HEX),
        originalSizeBytes: Long = ORIGINAL_SIZE_BYTES,
        compressedSizeBytes: Long = COMPRESSED_SIZE_BYTES,
    ): CatalogEntry =
        CatalogEntry(
            uploadId = UPLOAD_ID,
            sourceName = ORIGINAL_NAME,
            timestampMillis = TRACKED_TIME_MILLIS,
            originalSizeBytes = originalSizeBytes,
            compressedSizeBytes = compressedSizeBytes,
            sha256 = sha256,
        )

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) { index -> hex.substring(index * 2, index * 2 + 2).toInt(16).toByte() }

    private companion object {
        const val CONTENT_SHA256_HEX = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
        const val ORIGINAL_NAME = "photo.jpg"
        const val TRACKED_TIME_MILLIS = 1_700_000_000_000L
        const val ORIGINAL_SIZE_BYTES = 1_024L
        const val COMPRESSED_SIZE_BYTES = 512L
        const val SHA256_BYTES = 32
        val UPLOAD_ID = ByteArray(16) { index -> (index + 1).toByte() }
    }
}
