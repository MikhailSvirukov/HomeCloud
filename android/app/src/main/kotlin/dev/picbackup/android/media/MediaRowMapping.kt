package dev.picbackup.android.media

/**
 * Pure mapping from raw media-store column values to [RawMediaEntry].
 *
 * This deliberately depends on no `android.*` type, so the column-to-model
 * mapping can be exercised with plain JVM unit tests before MediaStore access
 * is wired up.
 */
object MediaRowMapping {

    /** Fallback for a MediaStore text column that was read as `null`. */
    private const val EMPTY_TEXT = ""

    /**
     * Maps one media-store row's column values to a [RawMediaEntry].
     *
     * MediaStore can report `null` for its text columns, so a null
     * [displayName] or [mimeType] is normalized to [EMPTY_TEXT] (`""`). An
     * empty mime type is intentionally left for the classifier
     * ([MediaCatalog]) to drop; this mapping never classifies or filters, and a
     * blank-but-not-null mime type is preserved exactly as given.
     *
     * @param id stable media store identifier, used for deterministic ordering.
     * @param uri content uri of the media item.
     * @param displayName human-readable display name reported by MediaStore, or `null`.
     * @param sizeBytes size of the original content in bytes.
     * @param dateModifiedEpochSeconds last modification time, in seconds since the epoch.
     * @param mimeType mime type reported by MediaStore, or `null`.
     * @return the mapped, still-unclassified row.
     */
    fun toRawMediaEntry(
        id: Long,
        uri: String,
        displayName: String?,
        sizeBytes: Long,
        dateModifiedEpochSeconds: Long,
        mimeType: String?,
    ): RawMediaEntry =
        RawMediaEntry(
            id = id,
            uri = uri,
            displayName = displayName ?: EMPTY_TEXT,
            sizeBytes = sizeBytes,
            dateModifiedEpochSeconds = dateModifiedEpochSeconds,
            mimeType = mimeType ?: EMPTY_TEXT,
        )
}
