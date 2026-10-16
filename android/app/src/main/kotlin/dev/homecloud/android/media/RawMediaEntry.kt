package dev.homecloud.android.media

/**
 * A raw MediaStore-like row before classification.
 *
 * Mirrors the columns read from `MediaStore` without depending on any `android.*`
 * type, so classification stays a pure JVM operation.
 */
data class RawMediaEntry(
    val id: Long,
    val uri: String,
    val displayName: String,
    val sizeBytes: Long,
    val dateModifiedEpochSeconds: Long,
    val mimeType: String,
)
