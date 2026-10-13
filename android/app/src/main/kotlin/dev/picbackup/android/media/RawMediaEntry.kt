package dev.picbackup.android.media

/**
 * A raw MediaStore-like row before it has been classified.
 *
 * This mirrors the columns the client will read from `MediaStore` without
 * depending on any `android.*` type, so classification stays a pure JVM
 * operation that can be unit tested before MediaStore access is implemented.
 *
 * @property id stable media store identifier, used for deterministic ordering.
 * @property uri content uri of the media item.
 * @property displayName human-readable display name reported by MediaStore.
 * @property sizeBytes size of the original content in bytes.
 * @property dateModifiedEpochSeconds last modification time, in seconds since the epoch.
 * @property mimeType mime type reported by MediaStore.
 */
data class RawMediaEntry(
    val id: Long,
    val uri: String,
    val displayName: String,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val mimeType: String,
)
