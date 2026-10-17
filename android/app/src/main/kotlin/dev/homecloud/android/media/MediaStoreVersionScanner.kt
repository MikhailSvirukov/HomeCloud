package dev.homecloud.android.media

import java.time.ZoneId

/**
 * [MediaVersionScanner] over the rows read from the device media store.
 *
 * Reads the media rows once and keeps only photos and videos, preserving the classifier's
 * id-ascending order (`FR-AND-003`). Each candidate's original bytes are opened exactly once and
 * hashed before compression or encryption (`FR-AND-007`), then the content hash, original display
 * name and tracked time form the version identity (`FR-AND-015`). The tracked time prefers the
 * file's metadata (`FR-AND-008`) and falls back to the `MediaStore` modification time
 * (`FR-AND-009`), with zone-less metadata interpreted in the device zone supplied at construction
 * (`FR-AND-011`).
 *
 * A candidate whose bytes cannot be opened or read fails the whole scan: the exception propagates
 * and no partial candidate list is returned.
 *
 * The media read and hashing are blocking, so callers must keep [scan] off the main thread.
 */
internal class MediaStoreVersionScanner(
    private val rows: MediaRowSource,
    private val content: MediaContentSource,
    private val metadataTime: MediaMetadataTimeSource,
    private val deviceZone: ZoneId,
) : MediaVersionScanner {
    /**
     * Reads all rows once and returns one candidate per discovered photo or video, in the
     * classifier's order.
     */
    override fun scan(): List<MediaVersionCandidate> {
        val items = MediaCatalog.backupCandidates(rows.readAll())
        return items.map(::versionCandidate)
    }

    private fun versionCandidate(item: MediaItem): MediaVersionCandidate {
        val contentSha256 = content.open(item.uri).use { stream -> MediaContentHasher.sha256Hex(stream) }
        val metadataCaptureTime = metadataTime.captureTime(item.uri, mimeTypeOf(item.kind))
        val trackedTimeMillis =
            CaptureTimeResolver.trackedTimeMillis(
                metadata = metadataCaptureTime,
                mediaStoreModifiedEpochSeconds = item.dateModifiedEpochSeconds,
                deviceZone = deviceZone,
            )
        val identity =
            MediaVersionIdentity(
                contentSha256 = contentSha256,
                originalName = item.displayName,
                trackedTimeMillis = trackedTimeMillis,
            )
        return MediaVersionCandidate(item = item, identity = identity)
    }

    /**
     * The media type family passed to the metadata source.
     *
     * A classified [MediaItem] keeps its kind but not its exact mime type, so the family wildcard
     * is the most specific value available at this layer.
     */
    private fun mimeTypeOf(kind: MediaKind): String =
        when (kind) {
            MediaKind.IMAGE -> IMAGE_MIME_TYPE_FAMILY
            MediaKind.VIDEO -> VIDEO_MIME_TYPE_FAMILY
        }

    private companion object {
        const val IMAGE_MIME_TYPE_FAMILY = "image/*"
        const val VIDEO_MIME_TYPE_FAMILY = "video/*"
    }
}
