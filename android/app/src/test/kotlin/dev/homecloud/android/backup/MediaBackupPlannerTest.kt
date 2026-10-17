package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.RawMediaEntry
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.testsupport.FakeMediaRowSource
import dev.homecloud.android.testsupport.FakeUploadIdFactory
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaBackupPlannerTest {

    @Test
    fun `plan enqueues every discovered image and video candidate on an empty queue`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val planner = plannerFor(
            rows = listOf(
                MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                MediaTestData.row(id = 2L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
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
        val source = FakeMediaRowSource(
            listOf(
                MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                MediaTestData.row(id = 2L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
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
            rows = listOf(MediaTestData.row(id = 1L, mimeType = MediaTestData.AUDIO_MIME_TYPE)),
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
                MediaTestData.row(
                    id = CANDIDATE_ID,
                    mimeType = MediaTestData.IMAGE_MIME_TYPE,
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
                MediaTestData.row(id = 1L, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                MediaTestData.row(id = 2L, mimeType = MediaTestData.VIDEO_MIME_TYPE),
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
        MediaBackupPlanner(MediaDiscovery(FakeMediaRowSource(rows)), uploadIdFactory, queue)

    private companion object {
        const val CANDIDATE_ID = 42L
        const val CANDIDATE_SIZE_BYTES = 4_096L
        const val CANDIDATE_DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_500L

        const val SHARED_UPLOAD_ID = "upload-shared"

        const val MAX_ATTEMPTS = 3
        const val EXPECTED_READ_COUNT_AFTER_TWO_PLANS = 2
    }
}
