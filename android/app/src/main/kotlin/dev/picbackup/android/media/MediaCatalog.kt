package dev.picbackup.android.media

/**
 * Pure classification of discovered media rows into backup candidates.
 *
 * Only photos and videos are ever candidates; audio is never queued. The
 * functions here perform no I/O and hold no state, so they can be exercised
 * with plain JVM unit tests before any MediaStore access is added.
 */
object MediaCatalog {

    private const val IMAGE_MIME_PREFIX = "image/"
    private const val VIDEO_MIME_PREFIX = "video/"

    /**
     * Classifies a raw mime type into a [MediaKind].
     *
     * Matching is case-insensitive and ignores surrounding whitespace.
     * A mime type that starts with [IMAGE_MIME_PREFIX] maps to [MediaKind.IMAGE],
     * one that starts with [VIDEO_MIME_PREFIX] maps to [MediaKind.VIDEO], and
     * everything else, notably the audio family, maps to `null` and is therefore
     * never queued for backup.
     *
     * @param mimeType mime type reported by MediaStore.
     * @return the matching [MediaKind], or `null` when the value is not a photo or video.
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
     * Maps [entries] to the backup candidates they contain.
     *
     * Each entry is classified with [classify]; entries that do not classify to a
     * [MediaKind] (audio, documents, blank or unknown mime types) are dropped.
     * The result is sorted by [MediaItem.id] ascending so the output is
     * deterministic regardless of the input order. Only photos and videos are
     * ever returned; audio is never queued.
     *
     * @param entries raw media rows to classify.
     * @return the classified candidates, sorted by id ascending.
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
                    )
                }
            }
            .sortedBy { it.id }
}
