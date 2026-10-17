package dev.homecloud.android.media

import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeFormatterBuilder
import java.time.format.DateTimeParseException
import java.time.format.ResolverStyle
import java.time.temporal.ChronoField
import java.util.Locale
import java.util.concurrent.TimeUnit

/**
 * A capture/creation time read from media metadata, together with the UTC offset the producer
 * recorded when it supplied one.
 *
 * A missing offset marks a zone-less timestamp, which is interpreted in the device zone captured
 * when the media library is scanned (`FR-AND-011`).
 */
internal data class MediaCaptureTime(
    val dateTime: LocalDateTime,
    val offset: ZoneOffset?,
)

/**
 * Resolves the tracked capture/creation time of a media file version.
 *
 * Metadata is the primary source (`FR-AND-008`) and the `MediaStore` modification time is the
 * fallback (`FR-AND-009`). The tracked time always reaches the queue as Unix milliseconds
 * (`FR-AND-010`) and is part of version identity (`FR-AND-014`, `FR-AND-015`).
 */
internal object CaptureTimeResolver {
    /**
     * Reads an EXIF-style capture timestamp such as `2024:05:01 12:34:56`.
     *
     * Both the colon-separated EXIF form and the dash-separated form some producers write are
     * accepted, including surrounding whitespace and an optional fractional second. Text that is
     * not a complete, valid calendar date and time carries no capture time.
     */
    fun parseDateTime(value: String): LocalDateTime? {
        val candidate = value.trim()
        if (candidate.isEmpty()) {
            return null
        }
        return DATE_TIME_FORMATTERS.firstNotNullOfOrNull { formatter ->
            parseDateTimeOrNull(candidate, formatter)
        }
    }

    /**
     * Reads an EXIF-style UTC offset such as `+03:00`, `-0530` or `Z`.
     *
     * The colon and compact hour/minute forms are accepted within the range a [ZoneOffset] can
     * represent. A blank or malformed value carries no offset, leaving the timestamp zone-less.
     */
    fun parseOffset(value: String?): ZoneOffset? {
        val candidate = value?.trim() ?: return null
        if (candidate == UTC_DESIGNATOR) {
            return ZoneOffset.UTC
        }
        val match = OFFSET_PATTERN.matchEntire(candidate) ?: return null
        val (sign, hoursText, minutesText) = match.destructured
        val hours = hoursText.toInt()
        val minutes = minutesText.toInt()
        if (minutes >= MINUTES_PER_HOUR) {
            return null
        }
        val totalSeconds = (hours * MINUTES_PER_HOUR + minutes) * SECONDS_PER_MINUTE
        if (totalSeconds > MAX_OFFSET_SECONDS) {
            return null
        }
        return ZoneOffset.ofTotalSeconds(if (sign == PLUS_SIGN) totalSeconds else -totalSeconds)
    }

    /**
     * Computes the tracked capture/creation time of a version in Unix milliseconds.
     *
     * Metadata wins whenever a timestamp was read (`FR-AND-008`); a timestamp with an offset
     * denotes that instant, while a zone-less timestamp is resolved in [deviceZone], the device
     * zone captured at scan time, so daylight-saving rules apply (`FR-AND-011`). Without metadata
     * the `MediaStore` modification time is converted from epoch seconds (`FR-AND-009`).
     * Realistic `MediaStore` seconds fit the millisecond range; impossible extremes saturate at
     * the `Long` bounds instead of wrapping, keeping the result a Unix millisecond timestamp
     * (`FR-AND-010`).
     */
    fun trackedTimeMillis(
        metadata: MediaCaptureTime?,
        mediaStoreModifiedEpochSeconds: Long,
        deviceZone: ZoneId,
    ): Long {
        if (metadata == null) {
            return TimeUnit.SECONDS.toMillis(mediaStoreModifiedEpochSeconds)
        }
        val offset = metadata.offset
        val instant =
            if (offset == null) {
                metadata.dateTime.atZone(deviceZone).toInstant()
            } else {
                metadata.dateTime.toInstant(offset)
            }
        return instant.toEpochMilli()
    }

    private fun parseDateTimeOrNull(
        candidate: String,
        formatter: DateTimeFormatter,
    ): LocalDateTime? =
        try {
            LocalDateTime.parse(candidate, formatter)
        } catch (exception: DateTimeParseException) {
            null
        }

    private fun dateTimeFormatter(dateSeparator: Char): DateTimeFormatter =
        DateTimeFormatterBuilder()
            .appendValue(ChronoField.YEAR, YEAR_DIGITS)
            .appendLiteral(dateSeparator)
            .appendValue(ChronoField.MONTH_OF_YEAR, DATE_FIELD_DIGITS)
            .appendLiteral(dateSeparator)
            .appendValue(ChronoField.DAY_OF_MONTH, DATE_FIELD_DIGITS)
            .appendLiteral(DATE_TIME_SEPARATOR)
            .appendValue(ChronoField.HOUR_OF_DAY, DATE_FIELD_DIGITS)
            .appendLiteral(TIME_SEPARATOR)
            .appendValue(ChronoField.MINUTE_OF_HOUR, DATE_FIELD_DIGITS)
            .appendLiteral(TIME_SEPARATOR)
            .appendValue(ChronoField.SECOND_OF_MINUTE, DATE_FIELD_DIGITS)
            .optionalStart()
            .appendFraction(ChronoField.NANO_OF_SECOND, MIN_FRACTION_DIGITS, MAX_FRACTION_DIGITS, true)
            .optionalEnd()
            .toFormatter(Locale.ROOT)
            .withResolverStyle(ResolverStyle.STRICT)

    private const val YEAR_DIGITS = 4
    private const val DATE_FIELD_DIGITS = 2
    private const val MIN_FRACTION_DIGITS = 1
    private const val MAX_FRACTION_DIGITS = 9
    private const val COLON_DATE_SEPARATOR = ':'
    private const val DASH_DATE_SEPARATOR = '-'
    private const val DATE_TIME_SEPARATOR = ' '
    private const val TIME_SEPARATOR = ':'
    private const val UTC_DESIGNATOR = "Z"
    private const val PLUS_SIGN = "+"
    private const val MINUTES_PER_HOUR = 60
    private const val SECONDS_PER_MINUTE = 60
    private const val MAX_OFFSET_HOURS = 18
    private const val MAX_OFFSET_SECONDS = MAX_OFFSET_HOURS * MINUTES_PER_HOUR * SECONDS_PER_MINUTE
    private val OFFSET_PATTERN = Regex("""([+-])(\d{2}):?(\d{2})""")
    private val DATE_TIME_FORMATTERS =
        listOf(
            dateTimeFormatter(COLON_DATE_SEPARATOR),
            dateTimeFormatter(DASH_DATE_SEPARATOR),
        )
}
