package dev.picbackup.android.media

/**
 * A classified backup candidate.
 *
 * A [RawMediaEntry] whose mime type matched a supported [MediaKind]. Only photos
 * and videos ever become candidates; audio never does.
 */
data class MediaItem(
    val id: Long,
    val uri: String,
    val displayName: String,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val kind: MediaKind,
)
