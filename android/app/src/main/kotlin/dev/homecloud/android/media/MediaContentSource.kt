package dev.homecloud.android.media

import java.io.InputStream

/**
 * Opens the original bytes of a media item.
 *
 * The stream must expose the file exactly as stored, before any compression or encryption, so a
 * version can be identified from its content (`FR-AND-007`). The caller owns the returned stream
 * and closes it.
 */
internal fun interface MediaContentSource {
    /**
     * Opens the original bytes identified by [uri].
     *
     * The open and subsequent reads are synchronous, so callers must keep them off the main
     * thread. A file that cannot be opened is reported by throwing, never as an empty stream.
     */
    fun open(uri: String): InputStream
}
