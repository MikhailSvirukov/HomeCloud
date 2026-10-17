package dev.homecloud.android.testsupport

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri

/**
 * A [ContentProvider] that records the arguments of the query it receives and serves a
 * fixed set of rows through a [RecordingMatrixCursor].
 */
internal class RecordingMediaContentProvider : ContentProvider() {
    /** Rows served to the next query, each in projection order. */
    var rows: List<Array<Any?>> = emptyList()

    /** Whether the next query returns no cursor, as an unreadable provider does. */
    var queryReturnsNull: Boolean = false

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
    ): Cursor? {
        lastUri = uri
        lastProjection = projection
        lastSelection = selection
        lastSelectionArgs = selectionArgs
        if (queryReturnsNull) return null
        val cursor = RecordingMatrixCursor(projection ?: emptyArray())
        rows.forEach { cursor.addRow(it) }
        lastCursor = cursor
        return cursor
    }

    override fun getType(uri: Uri): String? = null

    override fun insert(
        uri: Uri,
        values: ContentValues?,
    ): Uri? = null

    override fun delete(
        uri: Uri,
        selection: String?,
        selectionArgs: Array<String>?,
    ): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<String>?,
    ): Int = 0
}

/** A [MatrixCursor] that records whether it was closed, so a leaked cursor fails the test. */
internal class RecordingMatrixCursor(
    columns: Array<String>,
) : MatrixCursor(columns) {
    var closed: Boolean = false
        private set

    override fun close() {
        closed = true
        super.close()
    }
}
