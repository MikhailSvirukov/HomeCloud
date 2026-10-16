package dev.homecloud.android.media

import android.os.Build
import android.provider.MediaStore

/**
 * Builds the `MediaStore` query used to discover candidate media rows.
 *
 * Rows the media store is still writing carry `IS_PENDING = 1`; their size and
 * content are not final yet, so they are skipped until the store publishes them.
 * Rows the user trashed carry `IS_TRASHED = 1` and are on their way to deletion,
 * so they are not part of the live library. Both columns only exist on later
 * platform releases: the clauses naming them are produced solely for the API
 * levels that define them, because a query naming an unknown column fails.
 */
internal object MediaStoreQuery {

    /** Columns read for every candidate row. */
    val projection: Array<String> = arrayOf(
        MediaStore.MediaColumns._ID,
        MediaStore.MediaColumns.DISPLAY_NAME,
        MediaStore.MediaColumns.SIZE,
        MediaStore.MediaColumns.DATE_MODIFIED,
        MediaStore.MediaColumns.MIME_TYPE,
    )

    /** Mime type prefix identifying photos. */
    private const val IMAGE_MIME_PREFIX = "image/"

    /** Mime type prefix identifying videos. */
    private const val VIDEO_MIME_PREFIX = "video/"

    /** Selection arguments restricting the query to photos and videos only. */
    val selectionArgs: Array<String> = arrayOf(
        "$IMAGE_MIME_PREFIX%",
        "$VIDEO_MIME_PREFIX%",
    )

    /**
     * Restricts the query to photos and videos and, where the platform supports
     * the columns, to rows that are published and not trashed.
     */
    fun selection(sdkInt: Int): String {
        val mimeColumn = MediaStore.MediaColumns.MIME_TYPE
        val clauses = buildList {
            add("($mimeColumn LIKE ? OR $mimeColumn LIKE ?)")
            if (sdkInt >= Build.VERSION_CODES.Q) {
                add("${MediaStore.MediaColumns.IS_PENDING} = $CLEARED_STATE")
            }
            if (sdkInt >= Build.VERSION_CODES.R) {
                add("${MediaStore.MediaColumns.IS_TRASHED} = $CLEARED_STATE")
            }
        }
        return clauses.joinToString(" AND ")
    }

    /** The value both state columns hold for a row that is published and not trashed. */
    private const val CLEARED_STATE = "0"
}
