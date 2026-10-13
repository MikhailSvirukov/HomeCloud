package dev.picbackup.android.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaRowMappingTest {

    @Test
    fun `toRawMediaEntry maps every column of a fully populated row`() {
        val entry = MediaRowMapping.toRawMediaEntry(
            id = ID,
            uri = URI,
            displayName = DISPLAY_NAME,
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = IMAGE_MIME_TYPE,
        )

        assertEquals(ID, entry.id)
        assertEquals(URI, entry.uri)
        assertEquals(DISPLAY_NAME, entry.displayName)
        assertEquals(SIZE_BYTES, entry.sizeBytes)
        assertEquals(DATE_MODIFIED_EPOCH_SECONDS, entry.dateModifiedEpochSeconds)
        assertEquals(IMAGE_MIME_TYPE, entry.mimeType)
    }

    @Test
    fun `toRawMediaEntry normalizes a null mimeType to an empty string`() {
        val entry = MediaRowMapping.toRawMediaEntry(
            id = ID,
            uri = URI,
            displayName = DISPLAY_NAME,
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = null,
        )

        assertEquals(EMPTY_TEXT, entry.mimeType)
    }

    @Test
    fun `toRawMediaEntry normalizes a null displayName to an empty string`() {
        val entry = MediaRowMapping.toRawMediaEntry(
            id = ID,
            uri = URI,
            displayName = null,
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = IMAGE_MIME_TYPE,
        )

        assertEquals(EMPTY_TEXT, entry.displayName)
    }

    @Test
    fun `toRawMediaEntry preserves a blank but non-null mimeType as given`() {
        val entry = MediaRowMapping.toRawMediaEntry(
            id = ID,
            uri = URI,
            displayName = DISPLAY_NAME,
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = BLANK_MIME_TYPE,
        )

        assertEquals(BLANK_MIME_TYPE, entry.mimeType)
    }

    private companion object {
        const val ID = 42L
        const val URI = "content://media/external/images/media/42"
        const val DISPLAY_NAME = "photo-42.jpg"
        const val SIZE_BYTES = 2_048L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val BLANK_MIME_TYPE = "   "
        const val EMPTY_TEXT = ""
    }
}
