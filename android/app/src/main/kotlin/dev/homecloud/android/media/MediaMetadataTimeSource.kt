package dev.homecloud.android.media

/**
 * Reads the capture/create time a media item carries, if any.
 *
 * Metadata is the primary tracked-time source (`FR-AND-008`): the seam yields an image's Exif time
 * or the media-store taken time. A `null` result means neither source has a time, so the caller
 * falls back to the row's modification time (`FR-AND-009`). The media-store taken instant is
 * expressed in the device zone without normalisation. Reading is synchronous, so callers must keep
 * it off the main thread.
 */
internal fun interface MediaMetadataTimeSource {
    /**
     * Reads the capture/create time of [item], or `null` when neither its metadata nor the
     * media-store taken time reports one.
     */
    fun captureTime(item: MediaItem): MediaCaptureTime?
}
