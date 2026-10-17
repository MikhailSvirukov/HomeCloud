package dev.homecloud.android.media

import java.time.Instant
import java.time.LocalDateTime
import java.time.ZoneId

/**
 * Resolves the capture time a photo records in its EXIF attributes.
 *
 * A missing or unparseable original date carries no capture time. A present but unparseable UTC
 * offset leaves the time zone-less, so the device zone applies (`FR-AND-011`).
 */
internal fun exifCaptureTime(
    dateTimeOriginal: String?,
    offsetTimeOriginal: String?,
): MediaCaptureTime? {
    val dateTime = dateTimeOriginal?.let(CaptureTimeResolver::parseDateTime) ?: return null
    return MediaCaptureTime(
        dateTime = dateTime,
        offset = CaptureTimeResolver.parseOffset(offsetTimeOriginal),
    )
}

/**
 * [MediaMetadataTimeSource] that prefers a photo's Exif capture time and otherwise uses the
 * media-store taken time.
 *
 * Exif metadata is used when the app can read it (`FR-AND-008`); an unreadable or malformed stream
 * counts as no metadata instead of failing the scan. Without an Exif time the media-store taken
 * time applies (`FR-AND-009`), represented in [deviceZone] because the store records no offset
 * (`FR-AND-011`). Reading is synchronous, so callers must keep [captureTime] off the main thread.
 */
internal class ExifMediaMetadataTimeSource(
    private val exifReader: ExifDateTimeReader,
    private val deviceZone: ZoneId,
) : MediaMetadataTimeSource {
    /**
     * Reads the Exif time of a photo, or the item's store taken time, whichever exists.
     */
    override fun captureTime(item: MediaItem): MediaCaptureTime? =
        if (item.kind == MediaKind.IMAGE) {
            exifTimeOf(item) ?: mediaStoreTimeOf(item)
        } else {
            mediaStoreTimeOf(item)
        }

    private fun exifTimeOf(item: MediaItem): MediaCaptureTime? {
        val attributes = exifReader.read(item.uri) ?: return null
        return exifCaptureTime(
            dateTimeOriginal = attributes.dateTimeOriginal,
            offsetTimeOriginal = attributes.offsetTimeOriginal,
        )
    }

    private fun mediaStoreTimeOf(item: MediaItem): MediaCaptureTime? {
        val takenEpochMillis = item.dateTakenEpochMillis
        return if (takenEpochMillis != null && takenEpochMillis > 0) {
            val instant = Instant.ofEpochMilli(takenEpochMillis)
            MediaCaptureTime(
                dateTime = LocalDateTime.ofInstant(instant, deviceZone),
                offset = deviceZone.rules.getOffset(instant),
            )
        } else {
            null
        }
    }
}
