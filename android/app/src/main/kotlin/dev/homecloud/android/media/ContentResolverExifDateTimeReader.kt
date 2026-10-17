package dev.homecloud.android.media

import android.content.ContentResolver
import android.net.Uri
import androidx.exifinterface.media.ExifInterface
import java.io.IOException

/**
 * [ExifDateTimeReader] over a content resolver stream.
 *
 * A missing stream, an unreadable stream, or an Exif failure counts as no accessible attributes.
 * Every opened stream is closed here.
 */
internal class ContentResolverExifDateTimeReader(
    private val contentResolver: ContentResolver,
) : ExifDateTimeReader {
    override fun read(uri: String): ExifDateTimeAttributes? =
        try {
            contentResolver.openInputStream(Uri.parse(uri))?.use { stream ->
                val exif = ExifInterface(stream)
                ExifDateTimeAttributes(
                    dateTimeOriginal = exif.getAttribute(ExifInterface.TAG_DATETIME_ORIGINAL),
                    offsetTimeOriginal = exif.getAttribute(ExifInterface.TAG_OFFSET_TIME_ORIGINAL),
                )
            }
        } catch (_: IOException) {
            null
        } catch (_: SecurityException) {
            null
        }
}
