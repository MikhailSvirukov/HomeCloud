package dev.homecloud.android.media

/**
 * Pure mapping from raw media-store column values to [RawMediaEntry].
 *
 * Deliberately depends on no `android.*` type, so the mapping can be exercised
 * with plain JVM unit tests before MediaStore access is wired up.
 */
internal object MediaRowMapping {
    /** Fallback for a MediaStore text column that was read as `null`. */
    private const val EMPTY_TEXT = ""

    /**
     * Maps one row's column values to a [RawMediaEntry].
     *
     * A null text column is normalized to `""`; an empty mime type is left for
     * [MediaCatalog] to drop, so this mapping never classifies or filters.
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
