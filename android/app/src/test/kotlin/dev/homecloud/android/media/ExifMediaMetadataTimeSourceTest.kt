package dev.homecloud.android.media

import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset

/**
 * Exercises [ExifMediaMetadataTimeSource] against a fake [ExifDateTimeReader], so the adapter's
 * preference and fallback rules are asserted without depending on a readable Exif stream.
 */
class ExifMediaMetadataTimeSourceTest {
    @Test
    fun `exifCaptureTime parses a date with its recorded offset`() {
        val captureTime =
            exifCaptureTime(
                dateTimeOriginal = DATE_TIME_ORIGINAL,
                offsetTimeOriginal = OFFSET_TIME_ORIGINAL,
            )

        assertEquals(
            MediaCaptureTime(dateTime = LOCAL_DATE_TIME, offset = EXIF_ZONE_OFFSET),
            captureTime,
        )
    }

    @Test
    fun `exifCaptureTime leaves the offset null when the date has none`() {
        val captureTime = exifCaptureTime(dateTimeOriginal = DATE_TIME_ORIGINAL, offsetTimeOriginal = null)

        assertEquals(MediaCaptureTime(dateTime = LOCAL_DATE_TIME, offset = null), captureTime)
    }

    @Test
    fun `exifCaptureTime returns null for a missing date`() {
        assertNull(exifCaptureTime(dateTimeOriginal = null, offsetTimeOriginal = OFFSET_TIME_ORIGINAL))
    }

    @Test
    fun `exifCaptureTime returns null for an unparseable date`() {
        assertNull(exifCaptureTime(dateTimeOriginal = GARBAGE_DATE, offsetTimeOriginal = OFFSET_TIME_ORIGINAL))
    }

    @Test
    fun `an image prefers its Exif time over a different taken time`() {
        val source = source(reader = FakeExifDateTimeReader(EXIF_ATTRIBUTES))

        val captureTime = source.captureTime(imageItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertEquals(LOCAL_DATE_TIME, captureTime?.dateTime)
        assertEquals(EXIF_ZONE_OFFSET, captureTime?.offset)
        assertEquals(
            EXIF_EPOCH_MILLIS,
            captureTime?.dateTime?.toInstant(EXIF_ZONE_OFFSET)?.toEpochMilli(),
        )
    }

    @Test
    fun `an image whose Exif date carries no offset yields a null offset`() {
        val source = source(reader = FakeExifDateTimeReader(EXIF_ATTRIBUTES_WITHOUT_OFFSET))

        val captureTime = source.captureTime(imageItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertEquals(MediaCaptureTime(dateTime = LOCAL_DATE_TIME, offset = null), captureTime)
    }

    @Test
    fun `an image without Exif attributes falls back to its taken time`() {
        val source = source(reader = FakeExifDateTimeReader())

        val captureTime = source.captureTime(imageItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertEquals(TAKEN_LOCAL_DATE_TIME, captureTime?.dateTime)
        assertEquals(DEVICE_ZONE_SUMMER_OFFSET, captureTime?.offset)
        assertEquals(
            TAKEN_EPOCH_MILLIS,
            captureTime?.dateTime?.toInstant(DEVICE_ZONE_SUMMER_OFFSET)?.toEpochMilli(),
        )
    }

    @Test
    fun `a video ignores the Exif reader and uses its taken time`() {
        val reader = FakeExifDateTimeReader(EXIF_ATTRIBUTES)
        val source = source(reader = reader)

        val captureTime = source.captureTime(videoItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertTrue("a video must not be passed to the Exif reader", reader.readUris.isEmpty())
        assertEquals(TAKEN_LOCAL_DATE_TIME, captureTime?.dateTime)
        assertEquals(DEVICE_ZONE_SUMMER_OFFSET, captureTime?.offset)
    }

    @Test
    fun `an image without a positive taken time yields null`() {
        val source = source(reader = FakeExifDateTimeReader())

        listOf(null, 0L).forEach { takenEpochMillis ->
            assertNull(
                "taken time $takenEpochMillis must not become a capture time",
                source.captureTime(imageItem(dateTakenEpochMillis = takenEpochMillis)),
            )
        }
    }

    private fun source(reader: ExifDateTimeReader): ExifMediaMetadataTimeSource =
        ExifMediaMetadataTimeSource(exifReader = reader, deviceZone = DEVICE_ZONE)

    private fun videoItem(dateTakenEpochMillis: Long?): MediaItem =
        MediaItem(
            id = VIDEO_ID,
            uri = VIDEO_URI,
            displayName = VIDEO_DISPLAY_NAME,
            sizeBytes = MediaTestData.SIZE_BYTES,
            dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
            kind = MediaKind.VIDEO,
            dateTakenEpochMillis = dateTakenEpochMillis,
        )

    private fun imageItem(dateTakenEpochMillis: Long?): MediaItem =
        MediaItem(
            id = IMAGE_ID,
            uri = IMAGE_URI,
            displayName = IMAGE_DISPLAY_NAME,
            sizeBytes = MediaTestData.SIZE_BYTES,
            dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
            kind = MediaKind.IMAGE,
            dateTakenEpochMillis = dateTakenEpochMillis,
        )

    private companion object {
        const val DATE_TIME_ORIGINAL = "2021:03:04 05:06:07"
        const val OFFSET_TIME_ORIGINAL = "+02:00"
        const val GARBAGE_DATE = "not a capture time"
        val LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2021, 3, 4, 5, 6, 7)
        val EXIF_ZONE_OFFSET: ZoneOffset = ZoneOffset.ofHours(2)
        const val EXIF_EPOCH_MILLIS = 1_614_827_167_000L
        val EXIF_ATTRIBUTES =
            ExifDateTimeAttributes(
                dateTimeOriginal = DATE_TIME_ORIGINAL,
                offsetTimeOriginal = OFFSET_TIME_ORIGINAL,
            )
        val EXIF_ATTRIBUTES_WITHOUT_OFFSET =
            ExifDateTimeAttributes(dateTimeOriginal = DATE_TIME_ORIGINAL, offsetTimeOriginal = null)

        val DEVICE_ZONE: ZoneId = ZoneId.of("Europe/Berlin")
        val DEVICE_ZONE_SUMMER_OFFSET: ZoneOffset = ZoneOffset.ofHours(2)
        const val TAKEN_EPOCH_MILLIS = 1_689_319_541_000L

        val TAKEN_LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41)

        const val IMAGE_ID = 11L
        const val VIDEO_ID = 12L
        const val IMAGE_DISPLAY_NAME = "image-11.jpg"
        const val VIDEO_DISPLAY_NAME = "video-12.mp4"
        const val IMAGE_URI = "content://media/external/images/media/11"
        const val VIDEO_URI = "content://media/external/video/media/12"
    }
}

/** An [ExifDateTimeReader] that serves fixed attributes and records every uri it is asked about. */
private class FakeExifDateTimeReader(
    private val attributes: ExifDateTimeAttributes? = null,
) : ExifDateTimeReader {
    val readUris: MutableList<String> = mutableListOf()

    override fun read(uri: String): ExifDateTimeAttributes? {
        readUris += uri
        return attributes
    }
}
