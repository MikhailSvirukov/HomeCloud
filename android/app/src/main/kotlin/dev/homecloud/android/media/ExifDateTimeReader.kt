package dev.homecloud.android.media

/**
 * The capture-date attributes an image records in its Exif metadata.
 */
internal data class ExifDateTimeAttributes(
    val dateTimeOriginal: String?,
    val offsetTimeOriginal: String?,
)

/**
 * Reads the Exif capture-date attributes of an image; `null` when none are accessible.
 */
internal fun interface ExifDateTimeReader {
    fun read(uri: String): ExifDateTimeAttributes?
}
