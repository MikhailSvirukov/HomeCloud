package dev.homecloud.android.media

import android.content.ContentResolver
import android.net.Uri
import androidx.exifinterface.media.ExifInterface
import java.io.IOException
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
 * [MediaMetadataTimeSource] that prefers a photo's EXIF capture time and falls back to the
 * `MediaStore` taken time.
 *
 * EXIF metadata is used when the app can read it (`FR-AND-008`); an unreadable or malformed stream
 * counts as no metadata instead of failing the scan. Without an EXIF time the `MediaStore` taken
 * time applies (`FR-AND-009`), represented in [deviceZone] because the store records no offset
 * (`FR-AND-011`). Reading is synchronous, so callers must keep [captureTime] off the main thread.
 */
internal class ExifMediaMetadataTimeSource(
    private val contentResolver: ContentResolver,
    private val deviceZone: ZoneId,
) : MediaMetadataTimeSource {
    /**
     * Reads the EXIF time of a photo, or the item's store taken time, whichever exists.
     *
     * Only photos are opened for EXIF, and every opened stream is closed here.
     */
    override fun captureTime(item: MediaItem): MediaCaptureTime? =
        if (item.kind == MediaKind.IMAGE) {
            exifTimeOf(item) ?: mediaStoreTimeOf(item)
        } else {
            mediaStoreTimeOf(item)
        }

    private fun exifTimeOf(item: MediaItem): MediaCaptureTime? =
        try {
            contentResolver.openInputStream(Uri.parse(item.uri))?.use { stream ->
                val exif = ExifInterface(stream)
                exifCaptureTime(
                    dateTimeOriginal = exif.getAttribute(ExifInterface.TAG_DATETIME_ORIGINAL),
                    offsetTimeOriginal = exif.getAttribute(ExifInterface.TAG_OFFSET_TIME_ORIGINAL),
                )
            }
        } catch (_: IOException) {
            null
        } catch (_: SecurityException) {
            null
        }

    private fun mediaStoreTimeOf(item: MediaItem): MediaCaptureTime? {
        val takenEpochMillis = item.dateTakenEpochMillis ?: return null
        val instant = Instant.ofEpochMilli(takenEpochMillis)
        return MediaCaptureTime(
            dateTime = LocalDateTime.ofInstant(instant, deviceZone),
            offset = deviceZone.rules.getOffset(instant),
        )
    }
}
