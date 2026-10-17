package dev.homecloud.android.media

import android.content.ContentResolver
import android.content.ContentUris
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.MediaStore

/**
 * [MediaRowSource] that discovers media by reading the device `MediaStore`.
 *
 * Performs the actual `ContentResolver` query and translates each cursor row
 * into a [RawMediaEntry]; the selection restricts rows to photos and videos, so
 * audio and other media are never surfaced, and drops rows still being written
 * or already trashed on the platforms that expose those states.
 *
 * An empty cursor is a normal empty library. A query that returns no cursor is
 * a read failure reported as [MediaLibraryUnreadableException], never as an
 * empty library.
 *
 * The query is synchronous I/O through the supplied [contentResolver], so
 * callers must not invoke [readAll] on the main thread. The caller retains
 * ownership of the resolver's lifecycle.
 */
internal class MediaStoreMediaRowSource(
    private val contentResolver: ContentResolver,
) : MediaRowSource {
    /**
     * Reads every photo and video currently visible in the external `MediaStore`.
     *
     * Column indices are resolved once before iterating and each row is
     * translated by [MediaRowMapping.toRawMediaEntry]. Null `DISPLAY_NAME` and
     * `MIME_TYPE` values are passed through as `null` so normalization stays in
     * [MediaRowMapping.toRawMediaEntry]. The media store's taken time is read
     * when the cursor exposes it and stays `null` when the column is absent or
     * the row has no value (`FR-AND-009`). An empty cursor yields an empty list,
     * while a missing cursor is reported as [MediaLibraryUnreadableException].
     * Synchronous, so callers must keep it off the main thread.
     */
    override fun readAll(): List<RawMediaEntry> {
        val cursor =
            contentResolver.query(
                COLLECTION,
                MediaStoreQuery.projection,
                MediaStoreQuery.selection(Build.VERSION.SDK_INT),
                MediaStoreQuery.selectionArgs,
                null,
            ) ?: throw MediaLibraryUnreadableException("The media library query returned no cursor")

        return cursor.use { rows ->
            val idColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
            val displayNameColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
            val sizeColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
            val dateModifiedColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.DATE_MODIFIED)
            val mimeTypeColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.MIME_TYPE)
            val dateTakenColumn = rows.getColumnIndex(MediaStore.MediaColumns.DATE_TAKEN)

            val entries = mutableListOf<RawMediaEntry>()
            while (rows.moveToNext()) {
                val id = rows.getLong(idColumn)
                entries +=
                    MediaRowMapping.toRawMediaEntry(
                        id = id,
                        uri = ContentUris.withAppendedId(COLLECTION, id).toString(),
                        displayName = rows.getString(displayNameColumn),
                        sizeBytes = rows.getLong(sizeColumn),
                        dateModifiedEpochSeconds = rows.getLong(dateModifiedColumn),
                        dateTakenEpochMillis = takenEpochMillis(rows, dateTakenColumn),
                        mimeType = rows.getString(mimeTypeColumn),
                    )
            }
            entries
        }
    }

    private fun takenEpochMillis(
        cursor: Cursor,
        column: Int,
    ): Long? =
        if (column == ABSENT_COLUMN || cursor.isNull(column)) {
            null
        } else {
            cursor.getLong(column)
        }

    private companion object {
        /** The external `MediaStore.Files` collection shared by all volumes. */
        val COLLECTION: Uri = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)

        /** Column index a cursor reports when the projection did not expose the column. */
        const val ABSENT_COLUMN = -1
    }
}
