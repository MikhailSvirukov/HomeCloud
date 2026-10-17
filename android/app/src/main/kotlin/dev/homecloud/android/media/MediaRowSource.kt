package dev.homecloud.android.media

/**
 * The read seam over the device media store.
 *
 * Implementations return rows already restricted to photos and videos, so audio
 * and other media are never read and can never reach the classifier or queue.
 *
 * The read is deliberately plain and synchronous; callers are responsible for
 * not invoking it on the main thread.
 */
internal fun interface MediaRowSource {
    /**
     * Reads all rows; synchronous and may block, so callers must keep it off
     * the main thread.
     */
    fun readAll(): List<RawMediaEntry>
}
