package dev.picbackup.android.media

/**
 * The kinds of media this client is allowed to back up.
 *
 * Only photos and videos are ever candidates for backup. Audio and any other
 * media are deliberately not represented here, so they can never be classified
 * into the upload queue.
 */
enum class MediaKind {
    /** A still image, classified from a mime type with the image prefix. */
    IMAGE,

    /** A video, classified from a mime type with the video prefix. */
    VIDEO,
}
