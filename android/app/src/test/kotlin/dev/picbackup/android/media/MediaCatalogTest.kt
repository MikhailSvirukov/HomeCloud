package dev.picbackup.android.media

import dev.picbackup.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaCatalogTest {

    @Test
    fun `image mime classifies as IMAGE`() {
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(MediaTestData.IMAGE_MIME_TYPE))
    }

    @Test
    fun `video mime classifies as VIDEO`() {
        assertEquals(MediaKind.VIDEO, MediaCatalog.classify(MediaTestData.VIDEO_MIME_TYPE))
    }

    @Test
    fun `unsupported mime classifies as null`() {
        assertNull(MediaCatalog.classify(MediaTestData.AUDIO_MIME_TYPE))
        assertNull(MediaCatalog.classify(UNKNOWN_MIME_TYPE))
        assertNull(MediaCatalog.classify(BLANK_MIME_TYPE))
    }

    @Test
    fun `classify normalizes case and surrounding whitespace`() {
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(UPPERCASE_IMAGE_MIME_TYPE))
        assertEquals(MediaKind.VIDEO, MediaCatalog.classify(UPPERCASE_VIDEO_MIME_TYPE))
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(PADDED_IMAGE_MIME_TYPE))
    }

    @Test
    fun `backupCandidates keeps images and videos and drops audio and unknown`() {
        val entries = listOf(
            MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
            MediaTestData.row(id = 2L, mimeType = MediaTestData.AUDIO_MIME_TYPE),
            MediaTestData.row(id = 3L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
            MediaTestData.row(id = 4L, mimeType = UNKNOWN_MIME_TYPE),
        )

        val candidates = MediaCatalog.backupCandidates(entries)

        assertEquals(listOf(1L, 3L), candidates.map { it.id })
        assertEquals(listOf(MediaKind.IMAGE, MediaKind.VIDEO), candidates.map { it.kind })
    }

    @Test
    fun `backupCandidates sorts output by id ascending`() {
        val entries = listOf(
            MediaTestData.row(id = 30L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
            MediaTestData.row(id = 10L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
            MediaTestData.row(id = 20L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
        )

        val candidates = MediaCatalog.backupCandidates(entries)

        assertEquals(listOf(10L, 20L, 30L), candidates.map { it.id })
    }

    @Test
    fun `backupCandidates preserves source fields`() {
        val entry = MediaTestData.row(id = 7L, mimeType = MediaTestData.IMAGE_MIME_TYPE)

        val candidate = MediaCatalog.backupCandidates(listOf(entry)).single()

        assertEquals(entry.id, candidate.id)
        assertEquals(entry.uri, candidate.uri)
        assertEquals(entry.displayName, candidate.displayName)
        assertEquals(entry.sizeBytes, candidate.sizeBytes)
        assertEquals(entry.dateModifiedEpochSeconds, candidate.dateModifiedEpochSeconds)
    }

    private companion object {
        const val UNKNOWN_MIME_TYPE = "application/pdf"
        const val BLANK_MIME_TYPE = "   "
        const val UPPERCASE_IMAGE_MIME_TYPE = "IMAGE/PNG"
        const val UPPERCASE_VIDEO_MIME_TYPE = "VIDEO/WEBM"
        const val PADDED_IMAGE_MIME_TYPE = "  image/webp  "
    }
}
