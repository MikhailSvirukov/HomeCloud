package dev.picbackup.android.media

/**
 * Discovers the backup candidates currently present in the media store.
 *
 * Reads every raw row through the injected [rowSource] and keeps only what
 * [MediaCatalog.backupCandidates] classifies as photo or video. It performs no
 * caching, persistence, or change detection.
 */
class MediaDiscovery(private val rowSource: MediaRowSource) {

    /**
     * Reads all rows once through [rowSource] and returns the photo/video
     * candidates, sorted by [MediaItem.id] ascending.
     *
     * The underlying read is synchronous, so callers must keep this off the
     * main thread.
     */
    fun discoverBackupCandidates(): List<MediaItem> =
        MediaCatalog.backupCandidates(rowSource.readAll())
}
