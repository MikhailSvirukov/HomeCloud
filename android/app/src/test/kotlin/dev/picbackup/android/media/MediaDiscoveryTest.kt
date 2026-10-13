package dev.picbackup.android.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaDiscoveryTest {

    @Test
    fun `discoverBackupCandidates keeps image and video rows and drops audio`() {
        val source = RecordingMediaRowSource(
            listOf(
                rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 2L, mimeType = AUDIO_MIME_TYPE),
                rawEntry(id = 3L, mimeType = VIDEO_MIME_TYPE),
            ),
        )

        val candidates = MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(listOf(1L, 3L), candidates.map { it.id })
        assertEquals(listOf(MediaKind.IMAGE, MediaKind.VIDEO), candidates.map { it.kind })
    }

    @Test
    fun `discoverBackupCandidates sorts candidates by id ascending`() {
        val source = RecordingMediaRowSource(
            listOf(
                rawEntry(id = 30L, mimeType = VIDEO_MIME_TYPE),
                rawEntry(id = 10L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 20L, mimeType = IMAGE_MIME_TYPE),
            ),
        )

        val candidates = MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(listOf(10L, 20L, 30L), candidates.map { it.id })
    }

    @Test
    fun `discoverBackupCandidates of an empty source returns an empty list`() {
        val source = RecordingMediaRowSource(emptyList())

        val candidates = MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(emptyList<MediaItem>(), candidates)
    }

    @Test
    fun `discoverBackupCandidates reads the row source exactly once`() {
        val source = RecordingMediaRowSource(
            listOf(
                rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 2L, mimeType = VIDEO_MIME_TYPE),
            ),
        )

        MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(EXPECTED_READ_COUNT, source.readCount)
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

    private class RecordingMediaRowSource(
        private val rows: List<RawMediaEntry>,
    ) : MediaRowSource {

        var readCount: Int = 0
            private set

        override fun readAll(): List<RawMediaEntry> {
            readCount += 1
            return rows
        }
    }

    private companion object {
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val VIDEO_MIME_TYPE = "video/mp4"
        const val AUDIO_MIME_TYPE = "audio/mpeg"
        const val MEDIA_URI_PREFIX = "content://media/external/file/"
        const val DISPLAY_NAME_PREFIX = "item-"
        const val SIZE_BYTES = 1_024L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L
        const val EXPECTED_READ_COUNT = 1
    }
}
