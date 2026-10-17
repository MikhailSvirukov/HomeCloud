package dev.homecloud.android.media

import dev.homecloud.android.testsupport.FakeMediaRowSource
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Test

class MediaDiscoveryTest {
    @Test
    fun `discoverBackupCandidates of an empty source returns an empty list`() {
        val source = FakeMediaRowSource(emptyList())

        val candidates = MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(emptyList<MediaItem>(), candidates)
    }

    @Test
    fun `discoverBackupCandidates reads the row source exactly once`() {
        val source =
            FakeMediaRowSource(
                listOf(
                    MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                    MediaTestData.row(id = 2L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
                ),
            )

        MediaDiscovery(source).discoverBackupCandidates()

        assertEquals(EXPECTED_READ_COUNT, source.readCount)
    }

    private companion object {
        const val EXPECTED_READ_COUNT = 1
    }
}
