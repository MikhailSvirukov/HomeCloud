package dev.picbackup.android.media

/**
 * Discovers the backup candidates currently present in the media store.
 *
 * [MediaDiscovery] reads every raw row through the injected [rowSource] seam
 * and maps them through [MediaCatalog.backupCandidates], so non-photo/video
 * rows never reach the queue and the result is sorted by id. It performs no
 * caching, persistence, or change detection; those are deliberately later
 * concerns.
 */
class MediaDiscovery(private val rowSource: MediaRowSource) {

    /**
     * Reads all rows once through [rowSource] and returns the backup
     * candidates they contain.
     *
     * Only images and videos survive [MediaCatalog.backupCandidates]; audio and
     * unknown rows are dropped there. The result is sorted by [MediaItem.id]
     * ascending.
     *
     * [MediaRowSource.readAll] is synchronous, so callers are responsible for
     * not invoking this on the main thread.
     *
     * @return the photo/video candidates, sorted by id ascending.
     */
    fun discoverBackupCandidates(): List<MediaItem> =
        MediaCatalog.backupCandidates(rowSource.readAll())
}
