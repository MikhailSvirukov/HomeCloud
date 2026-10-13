package dev.picbackup.android.media

/**
 * A classified backup candidate.
 *
 * A [MediaItem] is a [RawMediaEntry] whose mime type matched a supported
 * [MediaKind]. Only photos and videos can become candidates; audio is never
 * classified into one.
 *
 * @property id stable media store identifier, used for deterministic ordering.
 * @property uri content uri of the media item.
 * @property displayName human-readable display name reported by MediaStore.
 * @property sizeBytes size of the original content in bytes.
 * @property dateModifiedEpochSeconds last modification time, in seconds since the epoch.
 * @property kind the classified media kind, never audio.
 */
data class MediaItem(
    val id: Long,
    val uri: String,
    val displayName: String,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val kind: MediaKind,
)
