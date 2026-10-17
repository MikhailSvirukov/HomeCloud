package dev.homecloud.android.media

import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Test

class MediaRowMappingTest {
    @Test
    fun `toRawMediaEntry maps every column of a fully populated row`() {
        val entry =
            MediaRowMapping.toRawMediaEntry(
                id = ID,
                uri = URI,
                displayName = DISPLAY_NAME,
                sizeBytes = MediaTestData.SIZE_BYTES,
                dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
                mimeType = MediaTestData.IMAGE_MIME_TYPE,
            )

        assertEquals(ID, entry.id)
        assertEquals(URI, entry.uri)
        assertEquals(DISPLAY_NAME, entry.displayName)
        assertEquals(MediaTestData.SIZE_BYTES, entry.sizeBytes)
        assertEquals(MediaTestData.DATE_MODIFIED_EPOCH_SECONDS, entry.dateModifiedEpochSeconds)
        assertEquals(MediaTestData.IMAGE_MIME_TYPE, entry.mimeType)
    }

    @Test
    fun `toRawMediaEntry normalizes null text columns to an empty string`() {
        val entry =
            MediaRowMapping.toRawMediaEntry(
                id = ID,
                uri = URI,
                displayName = null,
                sizeBytes = MediaTestData.SIZE_BYTES,
                dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
                mimeType = null,
            )

        assertEquals(EMPTY_TEXT, entry.displayName)
        assertEquals(EMPTY_TEXT, entry.mimeType)
    }

    @Test
    fun `toRawMediaEntry preserves a blank but non-null mimeType as given`() {
        val entry =
            MediaRowMapping.toRawMediaEntry(
                id = ID,
                uri = URI,
                displayName = DISPLAY_NAME,
                sizeBytes = MediaTestData.SIZE_BYTES,
                dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
                mimeType = BLANK_MIME_TYPE,
            )

        assertEquals(BLANK_MIME_TYPE, entry.mimeType)
    }

    private companion object {
        const val ID = 42L
        const val URI = "content://media/external/images/media/42"
        const val DISPLAY_NAME = "photo-42.jpg"
        const val BLANK_MIME_TYPE = "   "
        const val EMPTY_TEXT = ""
    }
}
