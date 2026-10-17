package dev.homecloud.android.media

/**
 * Reads the capture/create time a media item carries, if any.
 *
 * Metadata is the primary tracked-time source (`FR-AND-008`); a file whose metadata carries no
 * usable time yields `null` so the caller can fall back to the `MediaStore` time (`FR-AND-009`).
 * Reading is synchronous, so callers must keep it off the main thread.
 */
internal fun interface MediaMetadataTimeSource {
    /**
     * Reads the capture/create time of [item], or `null` when neither its metadata nor the
     * `MediaStore` reports one.
     */
    fun captureTime(item: MediaItem): MediaCaptureTime?
}
