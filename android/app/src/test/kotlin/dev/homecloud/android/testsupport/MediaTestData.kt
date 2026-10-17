package dev.homecloud.android.testsupport

import dev.homecloud.android.backup.UploadIdFactory
import dev.homecloud.android.media.MediaItem
import dev.homecloud.android.media.MediaRowSource
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
 * An [UploadIdFactory] that hands out pre-registered ids and falls back to a
 * deterministic id derived from the item id for anything not registered.
 */
internal class FakeUploadIdFactory(
    private val uploadIdsBySourceId: Map<Long, String> = emptyMap(),
) : UploadIdFactory {
    /** Maps a registered item id to its configured upload id, falling back to a deterministic id. */
    override fun uploadIdFor(item: MediaItem): String = uploadIdsBySourceId[item.id] ?: ("upload-" + item.id)
}
