package dev.homecloud.android.media

/** A discovered media item together with the identity of the version currently on the device. */
internal data class MediaVersionCandidate(
    val item: MediaItem,
    val identity: MediaVersionIdentity,
)

/** Reads the media store and computes the identity of each candidate (blocking; callers keep it off the main thread). */
internal fun interface MediaVersionScanner {
    fun scan(): List<MediaVersionCandidate>
}
