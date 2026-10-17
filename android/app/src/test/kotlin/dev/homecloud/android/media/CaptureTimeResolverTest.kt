package dev.homecloud.android.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset

class CaptureTimeResolverTest {
    @Test
    fun `parseDateTime parses the colon-separated EXIF form`() {
        assertEquals(COLON_DATE_TIME, CaptureTimeResolver.parseDateTime(COLON_FORM))
    }

    @Test
    fun `parseDateTime parses the dash-separated form`() {
        assertEquals(DASH_DATE_TIME, CaptureTimeResolver.parseDateTime(DASH_FORM))
    }

    @Test
    fun `parseDateTime parses optional fractional seconds`() {
        assertEquals(FRACTIONAL_DATE_TIME, CaptureTimeResolver.parseDateTime(FRACTIONAL_FORM))
    }

    @Test
    fun `parseDateTime trims surrounding whitespace`() {
        assertEquals(PADDED_DATE_TIME, CaptureTimeResolver.parseDateTime(PADDED_FORM))
    }

    @Test
    fun `parseDateTime returns null for blank and unparsable text`() {
        assertNull(CaptureTimeResolver.parseDateTime(BLANK_TEXT))
        assertNull(CaptureTimeResolver.parseDateTime(GARBAGE_TEXT))
    }

    @Test
    fun `parseDateTime returns null for a well-formed but impossible calendar date`() {
        assertNull(CaptureTimeResolver.parseDateTime(IMPOSSIBLE_DATE_FORM))
    }

    @Test
    fun `parseOffset parses signed hours and minutes in the colon form`() {
        assertEquals(POSITIVE_OFFSET, CaptureTimeResolver.parseOffset(POSITIVE_OFFSET_TEXT))
        assertEquals(NEGATIVE_OFFSET, CaptureTimeResolver.parseOffset(NEGATIVE_OFFSET_TEXT))
    }

    @Test
    fun `parseOffset parses the compact form`() {
        assertEquals(POSITIVE_OFFSET, CaptureTimeResolver.parseOffset(COMPACT_POSITIVE_OFFSET_TEXT))
    }

    @Test
    fun `parseOffset parses the Z designator`() {
        assertEquals(ZoneOffset.UTC, CaptureTimeResolver.parseOffset(ZULU_OFFSET_TEXT))
    }

    @Test
    fun `parseOffset returns null for missing blank and out-of-range text`() {
        assertNull(CaptureTimeResolver.parseOffset(null))
        assertNull(CaptureTimeResolver.parseOffset(BLANK_TEXT))
        assertNull(CaptureTimeResolver.parseOffset(OUT_OF_RANGE_HOURS_TEXT))
        assertNull(CaptureTimeResolver.parseOffset(OUT_OF_RANGE_MINUTES_TEXT))
    }

    @Test
    fun `trackedTimeMillis resolves a metadata time with an offset to its exact instant`() {
        val metadata =
            MediaCaptureTime(
                dateTime = FIXED_LOCAL_DATE_TIME,
                offset = ZoneOffset.ofHours(FIXED_LOCAL_OFFSET_HOURS),
            )

        val millis =
            CaptureTimeResolver.trackedTimeMillis(
                metadata = metadata,
                mediaStoreModifiedEpochSeconds = MEDIA_STORE_EPOCH_SECONDS,
                deviceZone = NEW_YORK_ZONE,
            )

        assertEquals(FIXED_INSTANT_EPOCH_MILLIS, millis)
    }

    @Test
    fun `trackedTimeMillis applies the device zone summer offset when metadata has no offset`() {
        val metadata =
            MediaCaptureTime(
                dateTime = BERLIN_SUMMER_LOCAL_DATE_TIME,
                offset = null,
            )

        val millis =
            CaptureTimeResolver.trackedTimeMillis(
                metadata = metadata,
                mediaStoreModifiedEpochSeconds = MEDIA_STORE_EPOCH_SECONDS,
                deviceZone = BERLIN_ZONE,
            )

        assertEquals(BERLIN_SUMMER_EPOCH_MILLIS, millis)
    }

    @Test
    fun `trackedTimeMillis applies the device zone winter offset when metadata has no offset`() {
        val metadata =
            MediaCaptureTime(
                dateTime = BERLIN_WINTER_LOCAL_DATE_TIME,
                offset = null,
            )

        val millis =
            CaptureTimeResolver.trackedTimeMillis(
                metadata = metadata,
                mediaStoreModifiedEpochSeconds = MEDIA_STORE_EPOCH_SECONDS,
                deviceZone = BERLIN_ZONE,
            )

        assertEquals(BERLIN_WINTER_EPOCH_MILLIS, millis)
    }

    @Test
    fun `trackedTimeMillis falls back to the media store seconds as milliseconds`() {
        val millis =
            CaptureTimeResolver.trackedTimeMillis(
                metadata = null,
                mediaStoreModifiedEpochSeconds = MEDIA_STORE_EPOCH_SECONDS,
                deviceZone = BERLIN_ZONE,
            )

        assertEquals(MEDIA_STORE_EPOCH_MILLIS, millis)
    }

    private companion object {
        const val COLON_FORM = "2021:03:04 05:06:07"
        val COLON_DATE_TIME: LocalDateTime = LocalDateTime.of(2021, 3, 4, 5, 6, 7)
        const val DASH_FORM = "2022-11-12 13:14:15"
        val DASH_DATE_TIME: LocalDateTime = LocalDateTime.of(2022, 11, 12, 13, 14, 15)
        const val FRACTIONAL_FORM = "2023:07:14 09:25:41.123"
        val FRACTIONAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41, 123_000_000)
        const val PADDED_FORM = "  2023:07:14 09:25:41  "
        val PADDED_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41)
        const val BLANK_TEXT = "   "
        const val GARBAGE_TEXT = "not a capture time"
        const val IMPOSSIBLE_DATE_FORM = "2023:02:30 09:25:41"

        val POSITIVE_OFFSET: ZoneOffset = ZoneOffset.ofHours(2)
        val NEGATIVE_OFFSET: ZoneOffset = ZoneOffset.ofHoursMinutes(-5, -30)
        const val POSITIVE_OFFSET_TEXT = "+02:00"
        const val NEGATIVE_OFFSET_TEXT = "-05:30"
        const val COMPACT_POSITIVE_OFFSET_TEXT = "+0200"
        const val ZULU_OFFSET_TEXT = "Z"
        const val OUT_OF_RANGE_HOURS_TEXT = "+19:00"
        const val OUT_OF_RANGE_MINUTES_TEXT = "+02:60"

        const val FIXED_LOCAL_OFFSET_HOURS = 2
        val FIXED_LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 1, 1, 2, 0)
        const val FIXED_INSTANT_EPOCH_MILLIS = 1_672_531_200_000L
        val BERLIN_ZONE: ZoneId = ZoneId.of("Europe/Berlin")
        val NEW_YORK_ZONE: ZoneId = ZoneId.of("America/New_York")
        val BERLIN_SUMMER_LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41)
        const val BERLIN_SUMMER_EPOCH_MILLIS = 1_689_319_541_000L
        val BERLIN_WINTER_LOCAL_DATE_TIME: LocalDateTime = LocalDateTime.of(2023, 1, 14, 9, 25, 41)
        const val BERLIN_WINTER_EPOCH_MILLIS = 1_673_684_741_000L
        const val MEDIA_STORE_EPOCH_SECONDS = 1_700_000_000L
        const val MEDIA_STORE_EPOCH_MILLIS = 1_700_000_000_000L
    }
}
