package dev.homecloud.android.media

import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset

/** Android 11, the API level the area's other Robolectric framework tests use. */
private const val TESTED_API = 30

/**
 * Exercises [ExifMediaMetadataTimeSource] and its EXIF parsing through Robolectric.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [TESTED_API])
@ConscryptMode(ConscryptMode.Mode.OFF)
class ExifMediaMetadataTimeSourceTest {
    @Test
    fun `exifCaptureTime parses a date with its recorded offset`() {
        val captureTime =
            exifCaptureTime(
                dateTimeOriginal = DATE_TIME_ORIGINAL,
                offsetTimeOriginal = OFFSET_TIME_ORIGINAL,
            )

        assertEquals(
            MediaCaptureTime(dateTime = LOCAL_DATE_TIME, offset = ZoneOffset.ofHours(OFFSET_HOURS)),
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
    fun `a video taken time maps to the instant in the device zone`() {
        val captureTime = source().captureTime(videoItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertEquals(TAKEN_LOCAL_DATE_TIME, captureTime?.dateTime)
        assertEquals(DEVICE_ZONE_SUMMER_OFFSET, captureTime?.offset)
        assertEquals(
            TAKEN_EPOCH_MILLIS,
            captureTime?.dateTime?.toInstant(DEVICE_ZONE_SUMMER_OFFSET)?.toEpochMilli(),
        )
    }

    @Test
    fun `a video without a taken time yields null`() {
        assertNull(source().captureTime(videoItem(dateTakenEpochMillis = null)))
    }

    @Test
    fun `an image whose Exif cannot be read falls back to its taken time`() {
        val captureTime = source().captureTime(imageItem(dateTakenEpochMillis = TAKEN_EPOCH_MILLIS))

        assertEquals(TAKEN_LOCAL_DATE_TIME, captureTime?.dateTime)
        assertEquals(DEVICE_ZONE_SUMMER_OFFSET, captureTime?.offset)
    }

    private fun source(): ExifMediaMetadataTimeSource =
        ExifMediaMetadataTimeSource(
            contentResolver = RuntimeEnvironment.getApplication().contentResolver,
            deviceZone = DEVICE_ZONE,
        )

    private fun videoItem(dateTakenEpochMillis: Long?): MediaItem =
        MediaItem(
            id = VIDEO_ID,
            uri = UNREGISTERED_VIDEO_URI,
            displayName = VIDEO_DISPLAY_NAME,
            sizeBytes = MediaTestData.SIZE_BYTES,
            dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
            kind = MediaKind.VIDEO,
            dateTakenEpochMillis = dateTakenEpochMillis,
        )

    private fun imageItem(dateTakenEpochMillis: Long?): MediaItem =
        MediaItem(
            id = IMAGE_ID,
            uri = UNREGISTERED_IMAGE_URI,
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
        const val OFFSET_HOURS = 2
        val LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2021, 3, 4, 5, 6, 7)

        val DEVICE_ZONE: ZoneId = ZoneId.of("Europe/Berlin")
        val DEVICE_ZONE_SUMMER_OFFSET: ZoneOffset = ZoneOffset.ofHours(2)
        const val TAKEN_EPOCH_MILLIS = 1_689_319_541_000L
        val TAKEN_LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41)

        const val IMAGE_ID = 11L
        const val VIDEO_ID = 12L
        const val IMAGE_DISPLAY_NAME = "image-11.jpg"
        const val VIDEO_DISPLAY_NAME = "video-12.mp4"
        const val UNREGISTERED_IMAGE_URI = "content://media/external/images/media/11"
        const val UNREGISTERED_VIDEO_URI = "content://media/external/video/media/12"
    }
}
