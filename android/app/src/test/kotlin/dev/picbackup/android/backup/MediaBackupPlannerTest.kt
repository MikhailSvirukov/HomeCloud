package dev.picbackup.android.backup

import dev.picbackup.android.media.MediaDiscovery
import dev.picbackup.android.media.MediaItem
import dev.picbackup.android.media.MediaRowSource
import dev.picbackup.android.media.RawMediaEntry
import dev.picbackup.android.queue.InMemoryUploadQueue
import dev.picbackup.android.queue.UploadQueueState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaBackupPlannerTest {

    @Test
    fun `plan enqueues every discovered image and video candidate on an empty queue`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val planner = plannerFor(
            rows = listOf(
                rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 2L, mimeType = VIDEO_MIME_TYPE),
            ),
            queue = queue,
        )

        val result = planner.plan()

        assertEquals(PlanResult(discoveredCandidates = 2, newlyEnqueued = 2), result)
        assertEquals(2, queue.entries().size)
        assertEquals(
            listOf(UploadQueueState.PENDING, UploadQueueState.PENDING),
            queue.entries().map { it.state },
        )
    }

    @Test
    fun `plan called twice does not re-enqueue the already tracked candidates`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val source = RecordingMediaRowSource(
            listOf(
                rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 2L, mimeType = VIDEO_MIME_TYPE),
            ),
        )
        val planner = MediaBackupPlanner(MediaDiscovery(source), FakeUploadIdFactory(), queue)
        planner.plan()

        val secondResult = planner.plan()

        assertEquals(0, secondResult.newlyEnqueued)
        assertEquals(2, queue.entries().size)
        assertEquals(EXPECTED_READ_COUNT_AFTER_TWO_PLANS, source.readCount)
    }

    @Test
    fun `plan with only audio rows enqueues nothing`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val planner = plannerFor(
            rows = listOf(rawEntry(id = 1L, mimeType = AUDIO_MIME_TYPE)),
            queue = queue,
        )

        val result = planner.plan()

        assertEquals(PlanResult(discoveredCandidates = 0, newlyEnqueued = 0), result)
        assertTrue(queue.entries().isEmpty())
    }

    @Test
    fun `plan enqueues the candidate source id size and modification time verbatim`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val planner = plannerFor(
            rows = listOf(
                rawEntry(
                    id = CANDIDATE_ID,
                    mimeType = IMAGE_MIME_TYPE,
                    sizeBytes = CANDIDATE_SIZE_BYTES,
                    dateModifiedEpochSeconds = CANDIDATE_DATE_MODIFIED_EPOCH_SECONDS,
                ),
            ),
            queue = queue,
        )

        planner.plan()

        val entry = queue.entries().single()
        assertEquals(CANDIDATE_ID, entry.sourceId)
        assertEquals(CANDIDATE_SIZE_BYTES, entry.sizeBytes)
        assertEquals(CANDIDATE_DATE_MODIFIED_EPOCH_SECONDS, entry.dateModifiedEpochSeconds)
    }

    @Test
    fun `plan enqueues a single entry when two candidates share an upload id`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val planner = plannerFor(
            rows = listOf(
                rawEntry(id = 1L, mimeType = IMAGE_MIME_TYPE),
                rawEntry(id = 2L, mimeType = VIDEO_MIME_TYPE),
            ),
            queue = queue,
            uploadIdFactory = FakeUploadIdFactory(
                mapOf(1L to SHARED_UPLOAD_ID, 2L to SHARED_UPLOAD_ID),
            ),
        )

        val result = planner.plan()

        assertEquals(PlanResult(discoveredCandidates = 2, newlyEnqueued = 1), result)
        assertEquals(1, queue.entries().size)
    }

    private fun plannerFor(
        rows: List<RawMediaEntry>,
        queue: InMemoryUploadQueue,
        uploadIdFactory: UploadIdFactory = FakeUploadIdFactory(),
    ): MediaBackupPlanner =
        MediaBackupPlanner(MediaDiscovery(RecordingMediaRowSource(rows)), uploadIdFactory, queue)

    private fun rawEntry(
        id: Long,
        mimeType: String,
        sizeBytes: Long = SIZE_BYTES,
        dateModifiedEpochSeconds: Long = DATE_MODIFIED_EPOCH_SECONDS,
    ): RawMediaEntry =
        RawMediaEntry(
            id = id,
            uri = "$MEDIA_URI_PREFIX$id",
            displayName = "$DISPLAY_NAME_PREFIX$id",
            sizeBytes = sizeBytes,
            dateModifiedEpochSeconds = dateModifiedEpochSeconds,
            mimeType = mimeType,
        )

    private class RecordingMediaRowSource(
        private val rows: List<RawMediaEntry>,
    ) : MediaRowSource {

        var readCount: Int = 0
            private set

        override fun readAll(): List<RawMediaEntry> {
            readCount += 1
            return rows
        }
    }

    private class FakeUploadIdFactory(
        private val uploadIdsBySourceId: Map<Long, String> = emptyMap(),
    ) : UploadIdFactory {

        override fun uploadIdFor(item: MediaItem): String =
            uploadIdsBySourceId[item.id] ?: "$UPLOAD_ID_PREFIX${item.id}"
    }

    private companion object {
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val VIDEO_MIME_TYPE = "video/mp4"
        const val AUDIO_MIME_TYPE = "audio/mpeg"

        const val MEDIA_URI_PREFIX = "content://media/external/file/"
        const val DISPLAY_NAME_PREFIX = "item-"
        const val SIZE_BYTES = 1_024L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L

        const val CANDIDATE_ID = 42L
        const val CANDIDATE_SIZE_BYTES = 4_096L
        const val CANDIDATE_DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_500L

        const val UPLOAD_ID_PREFIX = "upload-"
        const val SHARED_UPLOAD_ID = "upload-shared"

        const val MAX_ATTEMPTS = 3
        const val EXPECTED_READ_COUNT_AFTER_TWO_PLANS = 2
    }
}
