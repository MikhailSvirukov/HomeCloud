package dev.homecloud.android.media

/**
 * How many photo and video backup candidates the client can see.
 *
 * The counts are the classified [MediaKind]s of the discovered candidates, and [totalCount] is
 * their sum.
 */
internal data class MediaLibrarySummary(
    val photoCount: Int,
    val videoCount: Int,
) {
    /** The photos and videos together. */
    val totalCount: Int
        get() = photoCount + videoCount
}

/**
 * Counts the discovered candidates by their classified [MediaKind], so photos and videos are
 * reported separately.
 */
internal fun summarize(candidates: List<MediaItem>): MediaLibrarySummary {
    val countsByKind = candidates.groupingBy { it.kind }.eachCount()
    return MediaLibrarySummary(
        photoCount = countsByKind[MediaKind.IMAGE] ?: 0,
        videoCount = countsByKind[MediaKind.VIDEO] ?: 0,
    )
}
