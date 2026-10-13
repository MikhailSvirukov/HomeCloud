package dev.picbackup.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaCatalogTest {

    @Test
    fun `image mime classifies as IMAGE`() {
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(IMAGE_MIME_TYPE))
    }

    @Test
    fun `video mime classifies as VIDEO`() {
        assertEquals(MediaKind.VIDEO, MediaCatalog.classify(VIDEO_MIME_TYPE))
    }

    @Test
    fun `audio mime classifies as null`() {
        assertNull(MediaCatalog.classify(AUDIO_MIME_TYPE))
    }

    @Test
    fun `unknown mime classifies as null`() {
        assertNull(MediaCatalog.classify(UNKNOWN_MIME_TYPE))
    }

    @Test
    fun `blank mime classifies as null`() {
        assertNull(MediaCatalog.classify(BLANK_MIME_TYPE))
    }

    @Test
    fun `classify is case-insensitive`() {
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(UPPERCASE_IMAGE_MIME_TYPE))
        assertEquals(MediaKind.VIDEO, MediaCatalog.classify(UPPERCASE_VIDEO_MIME_TYPE))
    }

    @Test
    fun `classify ignores surrounding whitespace`() {
        assertEquals(MediaKind.IMAGE, MediaCatalog.classify(PADDED_IMAGE_MIME_TYPE))
    }

    @Test
    fun `backupCandidates keeps images and videos and drops audio and unknown`() {
        val entries = listOf(
            rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
            rawEntry(id = 2L, mimeType = AUDIO_MIME_TYPE),
            rawEntry(id = 3L, mimeType = VIDEO_MIME_TYPE),
            rawEntry(id = 4L, mimeType = UNKNOWN_MIME_TYPE),
        )

        val candidates = MediaCatalog.backupCandidates(entries)

        assertEquals(listOf(1L, 3L), candidates.map { it.id })
        assertEquals(listOf(MediaKind.IMAGE, MediaKind.VIDEO), candidates.map { it.kind })
    }

    @Test
    fun `backupCandidates sorts output by id ascending`() {
        val entries = listOf(
            rawEntry(id = 30L, mimeType = VIDEO_MIME_TYPE),
            rawEntry(id = 10L, mimeType = IMAGE_MIME_TYPE),
            rawEntry(id = 20L, mimeType = IMAGE_MIME_TYPE),
        )

        val candidates = MediaCatalog.backupCandidates(entries)

        assertEquals(listOf(10L, 20L, 30L), candidates.map { it.id })
    }

    @Test
    fun `backupCandidates preserves source fields`() {
        val entry = rawEntry(id = 7L, mimeType = IMAGE_MIME_TYPE)

        val candidate = MediaCatalog.backupCandidates(listOf(entry)).single()

        assertEquals(entry.id, candidate.id)
        assertEquals(entry.uri, candidate.uri)
        assertEquals(entry.displayName, candidate.displayName)
        assertEquals(entry.sizeBytes, candidate.sizeBytes)
        assertEquals(entry.dateModifiedEpochSeconds, candidate.dateModifiedEpochSeconds)
    }

    @Test
    fun `backupCandidates of only audio returns empty list`() {
        val entries = listOf(rawEntry(id = 1L, mimeType = AUDIO_MIME_TYPE))

        assertEquals(emptyList<MediaItem>(), MediaCatalog.backupCandidates(entries))
    }

    private fun rawEntry(id: Long, mimeType: String): RawMediaEntry =
        RawMediaEntry(
            id = id,
            uri = "$MEDIA_URI_PREFIX$id",
            displayName = "$DISPLAY_NAME_PREFIX$id",
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = mimeType,
        )

    private companion object {
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val VIDEO_MIME_TYPE = "video/mp4"
        const val AUDIO_MIME_TYPE = "audio/mpeg"
        const val UNKNOWN_MIME_TYPE = "application/pdf"
        const val BLANK_MIME_TYPE = "   "
        const val UPPERCASE_IMAGE_MIME_TYPE = "IMAGE/PNG"
        const val UPPERCASE_VIDEO_MIME_TYPE = "VIDEO/WEBM"
        const val PADDED_IMAGE_MIME_TYPE = "  image/webp  "
        const val MEDIA_URI_PREFIX = "content://media/external/file/"
        const val DISPLAY_NAME_PREFIX = "item-"
        const val SIZE_BYTES = 1_024L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L
    }
}
