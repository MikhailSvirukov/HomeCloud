package dev.homecloud.android.media

/**
 * The read seam over the device media store.
 *
 * Implementations return rows already restricted to photos and videos, so audio
 * and other media are never read and can never reach the classifier or queue.
 *
 * The read is deliberately plain and synchronous; callers are responsible for
 * not invoking it on the main thread.
 *
 * A read with no rows describes a normal empty library. A library that cannot
 * be read at all is a failure the caller must handle, never an empty library.
 */
internal fun interface MediaRowSource {
    /**
     * Reads all rows; synchronous and may block, so callers must keep it off
     * the main thread.
     *
     * An empty result means the library has no photos or videos. A library that
     * cannot be read is reported as [MediaLibraryUnreadableException].
     */
    fun readAll(): List<RawMediaEntry>
}

/** Signals that the device media library could not be read. */
internal class MediaLibraryUnreadableException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)
