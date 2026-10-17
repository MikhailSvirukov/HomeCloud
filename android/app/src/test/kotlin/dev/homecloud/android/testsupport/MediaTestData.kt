package dev.homecloud.android.testsupport

import dev.homecloud.android.media.MediaItem
import dev.homecloud.android.media.MediaKind
import dev.homecloud.android.media.MediaRowSource
import dev.homecloud.android.media.MediaVersionCandidate
import dev.homecloud.android.media.MediaVersionIdentity
import dev.homecloud.android.media.MediaVersionScanner
import dev.homecloud.android.media.RawMediaEntry

/**
 * Shared media fixtures so tests build rows and fake seams from one definition
 * instead of copying literals from each other.
 */
internal object MediaTestData {
    /** The mime type of a supported still image. */
    const val IMAGE_MIME_TYPE: String = "image/jpeg"

    /** The mime type of a supported video. */
    const val VIDEO_MIME_TYPE: String = "video/mp4"

    /** The mime type of an unsupported audio row. */
    const val AUDIO_MIME_TYPE: String = "audio/mpeg"

    private const val MEDIA_URI_PREFIX: String = "content://media/external/file/"

    private const val DISPLAY_NAME_PREFIX: String = "item-"

    private const val CONTENT_SHA256_PREFIX: String = "sha256-"

    private const val TRACKED_TIME_MILLIS: Long = 1_700_000_000_000

    /** The default size of a raw row in bytes. */
    const val SIZE_BYTES: Long = 1_024

    /** The default last-modified instant of a raw row, in epoch seconds. */
    const val DATE_MODIFIED_EPOCH_SECONDS: Long = 1_700_000_000

    /**
     * Builds a raw row whose uri and display name are derived from the given id,
     * so rows are distinguishable without repeating the literals in each test.
     */
    fun row(
        id: Long,
        mimeType: String,
        sizeBytes: Long = SIZE_BYTES,
        dateModifiedEpochSeconds: Long = DATE_MODIFIED_EPOCH_SECONDS,
    ): RawMediaEntry =
        RawMediaEntry(
            id = id,
            uri = MEDIA_URI_PREFIX + id,
            displayName = DISPLAY_NAME_PREFIX + id,
            sizeBytes = sizeBytes,
            dateModifiedEpochSeconds = dateModifiedEpochSeconds,
            mimeType = mimeType,
        )

    /**
     * Builds a version identity derived from the given source id, so candidates
     * are distinguishable without repeating the identity literals in each test.
     */
    fun versionIdentity(
        sourceId: Long,
        contentSha256: String = CONTENT_SHA256_PREFIX + sourceId,
        originalName: String = DISPLAY_NAME_PREFIX + sourceId,
        trackedTimeMillis: Long = TRACKED_TIME_MILLIS,
    ): MediaVersionIdentity =
        MediaVersionIdentity(
            contentSha256 = contentSha256,
            originalName = originalName,
            trackedTimeMillis = trackedTimeMillis,
        )

    /**
     * Builds a discovered candidate whose item and identity are derived from the
     * given id, unless a different identity is supplied.
     */
    fun candidate(
        id: Long,
        kind: MediaKind,
        sizeBytes: Long = SIZE_BYTES,
        dateModifiedEpochSeconds: Long = DATE_MODIFIED_EPOCH_SECONDS,
        identity: MediaVersionIdentity = versionIdentity(sourceId = id),
    ): MediaVersionCandidate =
        MediaVersionCandidate(
            item =
                MediaItem(
                    id = id,
                    uri = MEDIA_URI_PREFIX + id,
                    displayName = DISPLAY_NAME_PREFIX + id,
                    sizeBytes = sizeBytes,
                    dateModifiedEpochSeconds = dateModifiedEpochSeconds,
                    kind = kind,
                ),
            identity = identity,
        )
}

/**
 * A [MediaRowSource] that serves a fixed list and records how often it was read.
 */
internal class FakeMediaRowSource(
    private val rows: List<RawMediaEntry>,
) : MediaRowSource {
    /** Number of times [readAll] has been called. */
    var readCount: Int = 0
        private set

    override fun readAll(): List<RawMediaEntry> {
        readCount += 1
        return rows
    }
}

/**
 * A [MediaVersionScanner] that serves a fixed candidate list and records how
 * often it was scanned.
 */
internal class FakeMediaVersionScanner(
    private val candidates: List<MediaVersionCandidate> = emptyList(),
) : MediaVersionScanner {
    /** Number of times [scan] has been called. */
    var scanCount: Int = 0
        private set

    override fun scan(): List<MediaVersionCandidate> {
        scanCount += 1
        return candidates
    }
}
