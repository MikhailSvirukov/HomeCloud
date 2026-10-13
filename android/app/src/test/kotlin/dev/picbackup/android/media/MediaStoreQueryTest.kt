package dev.picbackup.android.media

import android.provider.MediaStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaStoreQueryTest {

    @Test
    fun `projection is the five media columns in query order`() {
        assertEquals(
            listOf(
                MediaStore.MediaColumns._ID,
                MediaStore.MediaColumns.DISPLAY_NAME,
                MediaStore.MediaColumns.SIZE,
                MediaStore.MediaColumns.DATE_MODIFIED,
                MediaStore.MediaColumns.MIME_TYPE,
            ),
            MediaStoreQuery.projection.toList(),
        )
    }

    @Test
    fun `selection args restrict the query to images and videos`() {
        assertEquals(
            listOf(IMAGE_MIME_SELECTION_ARG, VIDEO_MIME_SELECTION_ARG),
            MediaStoreQuery.selectionArgs.toList(),
        )
    }

    @Test
    fun `selection keeps the photo and video clause with two placeholders on every API level`() {
        TESTED_API_LEVELS.forEach { sdkInt ->
            assertPhotoVideoClause(MediaStoreQuery.selection(sdkInt), sdkInt)
        }
    }

    @Test
    fun `selection below the pending API names neither the pending nor the trashed column`() {
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
    fun `selection from the pending API constrains pending to zero and omits trashed`() {
        val selection = MediaStoreQuery.selection(PENDING_API)

        assertConstrainedToZero(selection, MediaStore.MediaColumns.IS_PENDING)
        assertFalse(
            "expected no trashed clause before its API in: $selection",
            selection.contains(MediaStore.MediaColumns.IS_TRASHED),
        )
    }

    @Test
    fun `selection from the trashed API constrains both pending and trashed to zero`() {
        val selection = MediaStoreQuery.selection(TRASHED_API)

        assertConstrainedToZero(selection, MediaStore.MediaColumns.IS_PENDING)
        assertConstrainedToZero(selection, MediaStore.MediaColumns.IS_TRASHED)
    }

    @Test
    fun `selection above the state APIs keeps the photo and video clause alongside both state clauses`() {
        val selection = MediaStoreQuery.selection(ABOVE_TRASHED_API)

        assertPhotoVideoClause(selection, ABOVE_TRASHED_API)
        assertConstrainedToZero(selection, MediaStore.MediaColumns.IS_PENDING)
        assertConstrainedToZero(selection, MediaStore.MediaColumns.IS_TRASHED)
    }

    private fun assertPhotoVideoClause(selection: String, sdkInt: Int) {
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

    private fun assertConstrainedToZero(selection: String, column: String) {
        assertTrue(
            "expected '$column' constrained to $CLEARED_STATE in: $selection",
            Regex("$column\\s*=\\s*$CLEARED_STATE").containsMatchIn(selection),
        )
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
        const val CLEARED_STATE = "0"

        const val IMAGE_MIME_SELECTION_ARG = "image/%"
        const val VIDEO_MIME_SELECTION_ARG = "video/%"
    }
}
