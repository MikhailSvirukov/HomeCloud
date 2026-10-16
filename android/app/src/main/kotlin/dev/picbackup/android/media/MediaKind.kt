package dev.picbackup.android.media

/**
 * Media kinds this client backs up: photos and videos only.
 *
 * Audio is deliberately absent, so it can never be classified into the queue.
 */
enum class MediaKind {
    /** A still image. */
    IMAGE,

    /** A video. */
    VIDEO,
}
