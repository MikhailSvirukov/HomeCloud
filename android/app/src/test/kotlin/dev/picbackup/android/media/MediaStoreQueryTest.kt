package dev.picbackup.android.media

import android.provider.MediaStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaStoreQueryTest {

    @Test
    fun `projection holds exactly the five expected media columns`() {
        val expectedColumns = setOf(
            MediaStore.MediaColumns._ID,
            MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE,
            MediaStore.MediaColumns.DATE_MODIFIED,
            MediaStore.MediaColumns.MIME_TYPE,
        )

        assertEquals(expectedColumns, MediaStoreQuery.projection.toSet())
        assertEquals(expectedColumns.size, MediaStoreQuery.projection.size)
    }

    @Test
    fun `selection keeps the mime clause with two placeholders at every tested API level`() {
        TESTED_API_LEVELS.forEach { sdkInt ->
            val selection = MediaStoreQuery.selection(sdkInt)

            assertTrue(
                "expected the mime column at API $sdkInt in: $selection",
                selection.contains(MediaStore.MediaColumns.MIME_TYPE),
            )
            assertEquals(
                "expected exactly $PHOTO_VIDEO_PLACEHOLDER_COUNT placeholders at API $sdkInt in: $selection",
                PHOTO_VIDEO_PLACEHOLDER_COUNT,
                selection.count { it == PLACEHOLDER },
            )
        }
    }

    @Test
    fun `selection below the pending API names neither state column`() {
        val selection = MediaStoreQuery.selection(BELOW_PENDING_API)

        assertFalse(
            "expected no pending clause in: $selection",
            selection.contains(MediaStore.MediaColumns.IS_PENDING),
        )
        assertFalse(
            "expected no trashed clause in: $selection",
            selection.contains(MediaStore.MediaColumns.IS_TRASHED),
        )
    }

    @Test
    fun `selection at the pending API constrains pending to zero and does not name trashed`() {
        val selection = MediaStoreQuery.selection(PENDING_API)

        assertTrue(
            "expected pending constrained to zero in: $selection",
            selection.contains("${MediaStore.MediaColumns.IS_PENDING} = 0"),
        )
        assertFalse(
            "expected no trashed clause in: $selection",
            selection.contains(MediaStore.MediaColumns.IS_TRASHED),
        )
    }

    @Test
    fun `selection from the trashed API appends both state clauses to the parenthesised mime clause`() {
        val expected = "(${MediaStore.MediaColumns.MIME_TYPE} LIKE ? OR ${MediaStore.MediaColumns.MIME_TYPE} LIKE ?)" +
            " AND ${MediaStore.MediaColumns.IS_PENDING} = 0" +
            " AND ${MediaStore.MediaColumns.IS_TRASHED} = 0"

        assertEquals(expected, MediaStoreQuery.selection(TRASHED_API))
        assertEquals(expected, MediaStoreQuery.selection(ABOVE_TRASHED_API))
    }

    private companion object {
        /** Last platform level before the pending column exists. */
        const val BELOW_PENDING_API = 28

        /** First platform level that exposes the pending column (Android 10). */
        const val PENDING_API = 29

        /** First platform level that exposes the trashed column (Android 11). */
        const val TRASHED_API = 30

        /** A platform level above every state-column boundary. */
        const val ABOVE_TRASHED_API = 33

        val TESTED_API_LEVELS = listOf(
            BELOW_PENDING_API,
            PENDING_API,
            TRASHED_API,
            ABOVE_TRASHED_API,
        )

        const val PLACEHOLDER = '?'
        const val PHOTO_VIDEO_PLACEHOLDER_COUNT = 2
    }
}
