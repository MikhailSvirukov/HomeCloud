package dev.homecloud.android.media

/**
 * Pure classification of discovered media rows into backup candidates.
 *
 * Only photos and videos are ever candidates; audio is never queued. The
 * functions here perform no I/O and hold no state.
 */
internal object MediaCatalog {
    private const val IMAGE_MIME_PREFIX = "image/"
    private const val VIDEO_MIME_PREFIX = "video/"

    /**
     * Classifies a raw mime type into a [MediaKind].
     *
     * Matching ignores case and surrounding whitespace; anything that is not an
     * image or video, notably audio, maps to `null`.
     */
    fun classify(mimeType: String): MediaKind? {
        val normalized = mimeType.trim().lowercase()
        return when {
            normalized.startsWith(IMAGE_MIME_PREFIX) -> MediaKind.IMAGE
            normalized.startsWith(VIDEO_MIME_PREFIX) -> MediaKind.VIDEO
            else -> null
        }
    }

    /**
     * Maps raw rows to the backup candidates they contain.
     *
     * Entries that classify to no [MediaKind] are dropped and the result is
     * sorted by [MediaItem.id] ascending.
     */
    fun backupCandidates(entries: List<RawMediaEntry>): List<MediaItem> =
        entries
            .mapNotNull { entry ->
                classify(entry.mimeType)?.let { kind ->
                    MediaItem(
                        id = entry.id,
                        uri = entry.uri,
                        displayName = entry.displayName,
                        sizeBytes = entry.sizeBytes,
                        dateModifiedEpochSeconds = entry.dateModifiedEpochSeconds,
                        kind = kind,
                        dateTakenEpochMillis = entry.dateTakenEpochMillis,
                    )
                }
            }.sortedBy { it.id }
}
