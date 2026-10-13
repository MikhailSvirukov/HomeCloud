package dev.picbackup.android.media

/**
 * The read seam over the device media store.
 *
 * Implementations provide raw rows: every call returns the media-store rows the
 * client is allowed to see, already restricted to photos and videos. Audio and
 * any other media are not read at all, so they can never reach the classifier
 * or the upload queue.
 *
 * The read call is deliberately plain and synchronous: there are no coroutines
 * or threads here. Callers are responsible for not invoking it on the main
 * thread.
 */
fun interface MediaRowSource {

    /**
     * Reads all available media rows.
     *
     * Only photos and videos are read. The call is synchronous and may perform
     * blocking I/O; callers are responsible for not running it on the main
     * thread.
     *
     * @return the raw rows, in media-store order; never `null`.
     */
    fun readAll(): List<RawMediaEntry>
}
