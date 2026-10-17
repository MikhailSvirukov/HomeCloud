package dev.homecloud.android.media

import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Exercises the pure classification of discovered candidates into the totals the
 * library summary reports, without any Android or content-provider dependency.
 */
class MediaLibrarySummaryTest {
    @Test
    fun `summarize counts photos and videos separately with the total as their sum`() {
        val candidates =
            MediaCatalog.backupCandidates(
                listOf(
                    MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                    MediaTestData.row(id = 2L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
                    MediaTestData.row(id = 3L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                ),
            )

        val summary = summarize(candidates)

        assertEquals(EXPECTED_PHOTO_COUNT, summary.photoCount)
        assertEquals(EXPECTED_VIDEO_COUNT, summary.videoCount)
        assertEquals(EXPECTED_PHOTO_COUNT + EXPECTED_VIDEO_COUNT, summary.totalCount)
    }

    @Test
    fun `summarize of no candidates reports zero photos and zero videos`() {
        val summary = summarize(emptyList())

        assertEquals(0, summary.photoCount)
        assertEquals(0, summary.videoCount)
        assertEquals(0, summary.totalCount)
    }

    private companion object {
        const val EXPECTED_PHOTO_COUNT = 2
        const val EXPECTED_VIDEO_COUNT = 1
    }
}
