package dev.homecloud.android.media

import android.content.ContentResolver
import android.net.Uri
import java.io.FileNotFoundException
import java.io.InputStream

/**
 * [MediaContentSource] that opens media bytes through the platform [ContentResolver].
 *
 * The returned stream exposes the original bytes before compression or encryption (`FR-AND-007`)
 * and stays owned by the caller; this source never closes it. Opening and reading are synchronous,
 * so callers must keep them off the main thread.
 */
internal class ContentResolverMediaContentSource(
    private val contentResolver: ContentResolver,
) : MediaContentSource {
    /**
     * Opens the bytes identified by [uri] through the resolver.
     *
     * When the resolver exposes no stream for the item, the open fails with
     * [FileNotFoundException] rather than yielding an empty stream.
     */
    override fun open(uri: String): InputStream =
        contentResolver.openInputStream(Uri.parse(uri))
            ?: throw FileNotFoundException("The content resolver returned no stream for the media item")
}
