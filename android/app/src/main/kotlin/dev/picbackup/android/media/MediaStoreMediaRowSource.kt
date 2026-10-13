package dev.picbackup.android.media

import android.content.ContentResolver
import android.content.ContentUris
import android.net.Uri
import android.provider.MediaStore

/**
 * [MediaRowSource] that discovers media by reading the device `MediaStore`.
 *
 * Performs the actual `ContentResolver` query and translates each cursor row
 * into a [RawMediaEntry]; the selection restricts rows to photos and videos, so
 * audio and other media are never surfaced.
 *
 * The query is synchronous I/O through the supplied [contentResolver], so
 * callers must not invoke [readAll] on the main thread. The caller retains
 * ownership of the resolver's lifecycle.
 */
class MediaStoreMediaRowSource(
    private val contentResolver: ContentResolver,
) : MediaRowSource {

    /**
     * Reads every photo and video currently visible in the external `MediaStore`.
     *
     * Column indices are resolved once before iterating and each row is
     * translated by [MediaRowMapping.toRawMediaEntry]. Null `DISPLAY_NAME` and
     * `MIME_TYPE` values are passed through as `null` so normalization stays in
     * [MediaRowMapping.toRawMediaEntry]. Synchronous, so callers must keep it
     * off the main thread.
     */
    override fun readAll(): List<RawMediaEntry> {
        val cursor = contentResolver.query(
            COLLECTION,
            PROJECTION,
            SELECTION,
            SELECTION_ARGS,
            null,
        ) ?: return emptyList()

        return cursor.use { rows ->
            val idColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
            val displayNameColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
            val sizeColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
            val dateModifiedColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.DATE_MODIFIED)
            val mimeTypeColumn = rows.getColumnIndexOrThrow(MediaStore.MediaColumns.MIME_TYPE)

            val entries = mutableListOf<RawMediaEntry>()
            while (rows.moveToNext()) {
                val id = rows.getLong(idColumn)
                entries += MediaRowMapping.toRawMediaEntry(
                    id = id,
                    uri = ContentUris.withAppendedId(COLLECTION, id).toString(),
                    displayName = rows.getString(displayNameColumn),
                    sizeBytes = rows.getLong(sizeColumn),
                    dateModifiedEpochSeconds = rows.getLong(dateModifiedColumn),
                    mimeType = rows.getString(mimeTypeColumn),
                )
            }
            entries
        }
    }

    private companion object {

        /** The external `MediaStore.Files` collection shared by all volumes. */
        val COLLECTION: Uri = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)

        /** Columns read for every candidate row; order matches the index lookups. */
        val PROJECTION: Array<String> = arrayOf(
            MediaStore.MediaColumns._ID,
            MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE,
            MediaStore.MediaColumns.DATE_MODIFIED,
            MediaStore.MediaColumns.MIME_TYPE,
        )

        /** Mime type prefix identifying photos. */
        const val IMAGE_MIME_PREFIX = "image/"

        /** Mime type prefix identifying videos. */
        const val VIDEO_MIME_PREFIX = "video/"

        /** Column pattern selection; both placeholders bind to the prefixes below. */
        val SELECTION =
            "${MediaStore.MediaColumns.MIME_TYPE} LIKE ? OR ${MediaStore.MediaColumns.MIME_TYPE} LIKE ?"

        /** Selection arguments restricting the query to photos and videos only. */
        val SELECTION_ARGS: Array<String> = arrayOf(
            "$IMAGE_MIME_PREFIX%",
            "$VIDEO_MIME_PREFIX%",
        )
    }
}
