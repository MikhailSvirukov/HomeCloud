package dev.homecloud.android.catalog

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the catalog comparison of `FR-CAT-003` and `FR-CAT-004`: only local versions whose
 * content, name and time all match a confirmed catalog entry are excluded.
 */
class CatalogComparisonTest {
    @Test
    fun `a fully matching version is excluded`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion()),
                catalog = listOf(catalogEntry()),
            )

        assertTrue(result.isEmpty())
    }

    @Test
    fun `a changed content hash includes the version`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion(contentSha256 = OTHER_CONTENT_HASH)),
                catalog = listOf(catalogEntry()),
            )

        assertArrayEquals(OTHER_CONTENT_HASH, result.single().contentSha256)
    }

    @Test
    fun `a changed name includes the version`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion(originalName = OTHER_ORIGINAL_NAME)),
                catalog = listOf(catalogEntry()),
            )

        assertEquals(OTHER_ORIGINAL_NAME, result.single().originalName)
    }

    @Test
    fun `a changed tracked time includes the version`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion(trackedTimeMillis = OTHER_TRACKED_TIME_MILLIS)),
                catalog = listOf(catalogEntry()),
            )

        assertEquals(OTHER_TRACKED_TIME_MILLIS, result.single().trackedTimeMillis)
    }

    @Test
    fun `an empty catalog returns every local version`() {
        val local = listOf(localVersion(), localVersion(originalName = OTHER_ORIGINAL_NAME))

        val result = CatalogComparison.missingVersions(local = local, catalog = emptyList())

        assertEquals(local, result)
    }

    @Test
    fun `an empty local list returns no versions`() {
        val result =
            CatalogComparison.missingVersions(
                local = emptyList(),
                catalog = listOf(catalogEntry()),
            )

        assertTrue(result.isEmpty())
    }

    @Test
    fun `local input order is preserved`() {
        val first = localVersion(originalName = FIRST_NAME)
        val second = localVersion(originalName = SECOND_NAME)
        val third = localVersion(originalName = THIRD_NAME)

        val result =
            CatalogComparison.missingVersions(
                local = listOf(first, second, third),
                catalog = listOf(catalogEntry(sourceName = SECOND_NAME)),
            )

        assertEquals(listOf(FIRST_NAME, THIRD_NAME), result.map { version -> version.originalName })
    }

    @Test
    fun `duplicate local identities produce one version`() {
        val result =
            CatalogComparison.missingVersions(
                local =
                    listOf(
                        localVersion(),
                        localVersion(contentSha256 = CONTENT_HASH.copyOf()),
                    ),
                catalog = emptyList(),
            )

        assertEquals(1, result.size)
        assertArrayEquals(CONTENT_HASH, result.single().contentSha256)
    }

    @Test
    fun `catalog sizes do not affect matching`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion()),
                catalog =
                    listOf(
                        catalogEntry(
                            originalSizeBytes = OTHER_ORIGINAL_SIZE_BYTES,
                            compressedSizeBytes = OTHER_COMPRESSED_SIZE_BYTES,
                        ),
                    ),
            )

        assertTrue(result.isEmpty())
    }

    @Test
    fun `a digest of a different length does not match`() {
        val result =
            CatalogComparison.missingVersions(
                local = listOf(localVersion()),
                catalog = listOf(catalogEntry(sha256 = CONTENT_HASH.copyOf(SHA256_BYTES - 1))),
            )

        assertEquals(1, result.size)
    }

    @Test
    fun `the text form omits the digest bytes`() {
        val text = localVersion().toString()

        assertFalse(text.lowercase().contains(CONTENT_HASH_HEX_PREFIX))
        assertTrue(text.contains(ORIGINAL_NAME))
    }

    private fun localVersion(
        contentSha256: ByteArray = CONTENT_HASH,
        originalName: String = ORIGINAL_NAME,
        trackedTimeMillis: Long = TRACKED_TIME_MILLIS,
    ) = LocalVersion(
        contentSha256 = contentSha256,
        originalName = originalName,
        trackedTimeMillis = trackedTimeMillis,
    )

    private fun catalogEntry(
        sha256: ByteArray = CONTENT_HASH,
        sourceName: String = ORIGINAL_NAME,
        timestampMillis: Long = TRACKED_TIME_MILLIS,
        originalSizeBytes: Long = ORIGINAL_SIZE_BYTES,
        compressedSizeBytes: Long = COMPRESSED_SIZE_BYTES,
    ) = CatalogEntry(
        uploadId = UPLOAD_ID,
        sourceName = sourceName,
        timestampMillis = timestampMillis,
        originalSizeBytes = originalSizeBytes,
        compressedSizeBytes = compressedSizeBytes,
        sha256 = sha256,
    )

    private companion object {
        const val SHA256_BYTES = 32
        const val UPLOAD_ID_BYTES = 16
        const val CONTENT_HASH_HEX_PREFIX = "00010203"
        const val ORIGINAL_NAME = "IMG_0001.jpg"
        const val OTHER_ORIGINAL_NAME = "IMG_0002.jpg"
        const val FIRST_NAME = "first.jpg"
        const val SECOND_NAME = "second.jpg"
        const val THIRD_NAME = "third.jpg"
        const val TRACKED_TIME_MILLIS = 1_700_000_000_000L
        const val OTHER_TRACKED_TIME_MILLIS = 1_700_000_000_001L
        const val ORIGINAL_SIZE_BYTES = 4_096L
        const val COMPRESSED_SIZE_BYTES = 1_024L
        const val OTHER_ORIGINAL_SIZE_BYTES = 2_048L
        const val OTHER_COMPRESSED_SIZE_BYTES = 512L

        val CONTENT_HASH = ByteArray(SHA256_BYTES) { index -> index.toByte() }
        val OTHER_CONTENT_HASH = ByteArray(SHA256_BYTES) { index -> (index + 1).toByte() }
        val UPLOAD_ID = ByteArray(UPLOAD_ID_BYTES) { index -> index.toByte() }
    }
}
