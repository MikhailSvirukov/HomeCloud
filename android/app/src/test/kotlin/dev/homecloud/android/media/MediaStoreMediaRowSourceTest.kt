package dev.homecloud.android.media

import android.content.ContentProvider
import android.content.ContentResolver
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.provider.MediaStore
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Android 11: the first API level whose selection carries both the pending and the
 * trashed clauses, so the asserted selection is the full one.
 */
private const val TRASHED_API = 30

/** Last API level before Android 10, where neither state column is defined. */
private const val BELOW_PENDING_API = 28

/**
 * Exercises [MediaStoreMediaRowSource] end to end against a fake `ContentProvider`
 * installed for the `media` authority, so the real `ContentResolver` routes the source's
 * query to the fake instead of the device media store.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native
 * library is impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [TRASHED_API])
@ConscryptMode(ConscryptMode.Mode.OFF)
class MediaStoreMediaRowSourceTest {

    @Test
    fun `readAll queries with the projection selection and arguments of the media store query`() {
        val provider = installProvider(emptyList())

        MediaStoreMediaRowSource(resolver()).readAll()

        assertArrayEquals(MediaStoreQuery.projection, provider.lastProjection)
        assertEquals(MediaStoreQuery.selection(TRASHED_API), provider.lastSelection)
        assertArrayEquals(MediaStoreQuery.selectionArgs, provider.lastSelectionArgs)
        assertEquals(collectionUri(), provider.lastUri)
    }

    @Test
    fun `readAll maps each column to the raw entry and normalizes null text columns`() {
        installProvider(
            listOf(
                arrayOf(ID, DISPLAY_NAME, SIZE_BYTES, DATE_MODIFIED_EPOCH_SECONDS, IMAGE_MIME_TYPE),
                arrayOf(NULL_TEXT_ID, null, NULL_TEXT_SIZE_BYTES, NULL_TEXT_DATE_MODIFIED, null),
            ),
        )

        val entries = MediaStoreMediaRowSource(resolver()).readAll()

        assertEquals(EXPECTED_ENTRY_COUNT, entries.size)
        assertEquals(ID, entries[0].id)
        assertEquals(DISPLAY_NAME, entries[0].displayName)
        assertEquals(SIZE_BYTES, entries[0].sizeBytes)
        assertEquals(DATE_MODIFIED_EPOCH_SECONDS, entries[0].dateModifiedEpochSeconds)
        assertEquals(IMAGE_MIME_TYPE, entries[0].mimeType)
        assertEquals(NULL_TEXT_ID, entries[1].id)
        assertEquals(EMPTY_TEXT, entries[1].displayName)
        assertEquals(NULL_TEXT_SIZE_BYTES, entries[1].sizeBytes)
        assertEquals(NULL_TEXT_DATE_MODIFIED, entries[1].dateModifiedEpochSeconds)
        assertEquals(EMPTY_TEXT, entries[1].mimeType)
    }

    @Test
    fun `readAll builds each entry uri from the collection uri and the row id`() {
        installProvider(
            listOf(
                arrayOf(URI_ROW_ID, DISPLAY_NAME, SIZE_BYTES, DATE_MODIFIED_EPOCH_SECONDS, IMAGE_MIME_TYPE),
            ),
        )

        val entry = MediaStoreMediaRowSource(resolver()).readAll().single()

        assertEquals("${collectionUri()}/$URI_ROW_ID", entry.uri)
    }

    @Test
    fun `readAll returns an empty list when the provider has no rows`() {
        installProvider(emptyList())

        val entries = MediaStoreMediaRowSource(resolver()).readAll()

        assertTrue(entries.isEmpty())
    }

    @Test
    fun `readAll closes the cursor when the provider has no rows`() {
        val provider = installProvider(emptyList())

        MediaStoreMediaRowSource(resolver()).readAll()

        val cursor = provider.lastCursor
        assertTrue("expected the provider cursor to be closed", cursor != null && cursor.closed)
    }

    @Test
    @Config(sdk = [BELOW_PENDING_API])
    fun `readAll uses the mime-only selection below the pending API level`() {
        val provider = installProvider(emptyList())

        MediaStoreMediaRowSource(resolver()).readAll()

        assertEquals(MediaStoreQuery.selection(BELOW_PENDING_API), provider.lastSelection)
    }

    private fun installProvider(rows: List<Array<Any?>>): RecordingMediaContentProvider {
        val authority = requireNotNull(collectionUri().authority) {
            "media collection uri must carry an authority"
        }
        val installed = Robolectric.buildContentProvider(RecordingMediaContentProvider::class.java)
            .create(authority)
            .get()
        installed.rows = rows
        return installed
    }

    private fun resolver(): ContentResolver = RuntimeEnvironment.getApplication().contentResolver

    private fun collectionUri(): Uri = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)

    private companion object {
        const val EXPECTED_ENTRY_COUNT = 2
        const val ID = 7L
        const val NULL_TEXT_ID = 9L
        const val URI_ROW_ID = 17L
        const val DISPLAY_NAME = "photo-7.jpg"
        const val SIZE_BYTES = 2_048L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L
        const val NULL_TEXT_SIZE_BYTES = 5_120L
        const val NULL_TEXT_DATE_MODIFIED = 1_700_000_500L
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val EMPTY_TEXT = ""
    }
}

/**
 * A [ContentProvider] that records the arguments of the query it receives and serves a
 * fixed set of rows through a [RecordingMatrixCursor].
 */
internal class RecordingMediaContentProvider : ContentProvider() {

    /** Rows served to the next query, each in projection order. */
    var rows: List<Array<Any?>> = emptyList()

    var lastUri: Uri? = null
        private set

    var lastProjection: Array<String>? = null
        private set

    var lastSelection: String? = null
        private set

    var lastSelectionArgs: Array<String>? = null
        private set

    var lastCursor: RecordingMatrixCursor? = null
        private set

    override fun onCreate(): Boolean = true

    override fun query(
        uri: Uri,
        projection: Array<String>?,
        selection: String?,
        selectionArgs: Array<String>?,
        sortOrder: String?,
    ): Cursor {
        lastUri = uri
        lastProjection = projection
        lastSelection = selection
        lastSelectionArgs = selectionArgs
        val cursor = RecordingMatrixCursor(projection ?: emptyArray())
        rows.forEach { cursor.addRow(it) }
        lastCursor = cursor
        return cursor
    }

    override fun getType(uri: Uri): String? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<String>?,
    ): Int = 0
}

/** A [MatrixCursor] that records whether it was closed, so a leaked cursor fails the test. */
internal class RecordingMatrixCursor(columns: Array<String>) : MatrixCursor(columns) {

    var closed: Boolean = false
        private set

    override fun close() {
        closed = true
        super.close()
    }
}
