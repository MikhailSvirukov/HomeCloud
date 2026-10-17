package dev.homecloud.android.media

/**
 * Reads the capture/create time a media file's metadata carries, if any.
 *
 * Metadata is the primary tracked-time source (`FR-AND-008`); a file whose metadata carries no
 * usable time yields `null` so the caller can fall back to the `MediaStore` time (`FR-AND-009`).
 * Reading is synchronous, so callers must keep it off the main thread.
 */
internal fun interface MediaMetadataTimeSource {
    /**
     * Reads the metadata time of the media file at [uri], interpreted as a file of the
     * [mimeType] family, or `null` when the metadata carries no usable time.
     */
    fun captureTime(
        uri: String,
        mimeType: String,
    ): MediaCaptureTime?
}
