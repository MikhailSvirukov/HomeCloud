package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.RawMediaEntry
import dev.homecloud.android.queue.FileUploadQueueStore
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.queue.UploadQueueStore
import dev.homecloud.android.queue.UploadQueueStoreException
import dev.homecloud.android.testsupport.FakeMediaRowSource
import dev.homecloud.android.testsupport.FakeUploadIdFactory
import dev.homecloud.android.testsupport.MediaTestData
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class BackupCycleRunnerTest {

    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun `runOnce over an empty store enqueues every discovered image and video candidate`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val runner = runnerFor(store = FileUploadQueueStore(storeFile()), queue = queue)

        val result = runner.runOnce()

        assertEquals(
            PlanResult(discoveredCandidates = CANDIDATE_COUNT, newlyEnqueued = CANDIDATE_COUNT),
            result,
        )
        assertEquals(
            listOf("$UPLOAD_ID_PREFIX$IMAGE_ID", "$UPLOAD_ID_PREFIX$VIDEO_ID"),
            queue.entries().map { entry -> entry.uploadId },
        )
    }

    @Test
    fun `runOnce persists exactly the planned queue entries to the store`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val store = FileUploadQueueStore(storeFile())
        val runner = runnerFor(store = store, queue = queue)

        runner.runOnce()

        val stored = store.read()
        assertEquals(queue.entries(), stored)
    }

    @Test
    fun `runOnce loads an existing snapshot before planning and preserves its state and attempt count`() {
        val store = FileUploadQueueStore(storeFile())
        val existing =
            entry(
                uploadId = "$UPLOAD_ID_PREFIX$CANDIDATE_ID",
                state = UploadQueueState.FAILED,
                attemptCount = STORED_ATTEMPT_COUNT,
            )
        store.write(listOf(existing))
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val runner =
            runnerFor(
                store = store,
                queue = queue,
                rows = listOf(MediaTestData.row(id = CANDIDATE_ID, mimeType = MediaTestData.IMAGE_MIME_TYPE)),
            )

        val result = runner.runOnce()

        assertEquals(PlanResult(discoveredCandidates = SINGLE_CANDIDATE_COUNT, newlyEnqueued = 0), result)
        val queued = queue.entries().single()
        assertEquals(UploadQueueState.FAILED, queued.state)
        assertEquals(STORED_ATTEMPT_COUNT, queued.attemptCount)
        assertEquals(listOf(existing), store.read())
    }

    @Test
    fun `runOnce over the same store twice enqueues nothing new and keeps one entry per candidate`() {
        val store = FileUploadQueueStore(storeFile())
        val firstQueue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val secondQueue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val firstResult = runnerFor(store = store, queue = firstQueue).runOnce()
        val secondResult = runnerFor(store = store, queue = secondQueue).runOnce()

        assertEquals(PlanResult(CANDIDATE_COUNT, CANDIDATE_COUNT), firstResult)
        assertEquals(0, secondResult.newlyEnqueued)
        assertEquals(CANDIDATE_COUNT, secondResult.discoveredCandidates)
        assertEquals(firstQueue.entries(), secondQueue.entries())
        assertEquals(CANDIDATE_COUNT, store.read().size)
    }

    @Test
    fun `runOnce with no discovered candidates reports an empty plan and stores an empty snapshot`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val store = FileUploadQueueStore(storeFile())
        val runner = runnerFor(store = store, queue = queue, rows = emptyList())

        val result = runner.runOnce()

        assertEquals(PlanResult(discoveredCandidates = 0, newlyEnqueued = 0), result)
        assertTrue(store.read().isEmpty())
    }

    @Test
    fun `runOnce propagates a store read failure and writes nothing`() {
        val failure = UploadQueueStoreException(READ_FAILURE_MESSAGE)
        val store = FakeUploadQueueStore(readFailure = failure)
        val runner = runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS))

        val thrown = assertThrows(UploadQueueStoreException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
        assertEquals(0, store.writeCount)
    }

    @Test
    fun `runOnce propagates a store write failure`() {
        val failure = UploadQueueStoreException(WRITE_FAILURE_MESSAGE)
        val store = FakeUploadQueueStore(writeFailure = failure)
        val runner = runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS))

        val thrown = assertThrows(UploadQueueStoreException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
    }

    private fun runnerFor(
        store: UploadQueueStore,
        queue: InMemoryUploadQueue,
        rows: List<RawMediaEntry> = imageAndVideoRows(),
    ): BackupCycleRunner {
        val planner =
            MediaBackupPlanner(MediaDiscovery(FakeMediaRowSource(rows)), FakeUploadIdFactory(), queue)
        return BackupCycleRunner(planner = planner, store = store)
    }

    private fun imageAndVideoRows(): List<RawMediaEntry> =
        listOf(
            MediaTestData.row(id = IMAGE_ID, mimeType = MediaTestData.IMAGE_MIME_TYPE),
            MediaTestData.row(id = VIDEO_ID, mimeType = MediaTestData.VIDEO_MIME_TYPE),
        )

    private fun entry(
        uploadId: String,
        state: UploadQueueState,
        attemptCount: Int = 0,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = CANDIDATE_ID,
            sizeBytes = MediaTestData.SIZE_BYTES,
            dateModifiedEpochSeconds = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
            attemptCount = attemptCount,
            state = state,
        )

    private fun storeFile(): File = File(temporaryFolder.newFolder(), STORE_FILE_NAME)

    private class FakeUploadQueueStore(
        private val readFailure: UploadQueueStoreException? = null,
        private val writeFailure: UploadQueueStoreException? = null,
    ) : UploadQueueStore {

        var writeCount: Int = 0
            private set

        override fun read(): List<UploadQueueEntry> =
            readFailure?.let { failure -> throw failure } ?: emptyList()

        override fun write(entries: List<UploadQueueEntry>) {
            writeCount += 1
            writeFailure?.let { failure -> throw failure }
        }
    }

    private companion object {
        const val IMAGE_ID = 1L
        const val VIDEO_ID = 2L
        const val CANDIDATE_ID = 3L
        const val CANDIDATE_COUNT = 2
        const val SINGLE_CANDIDATE_COUNT = 1

        const val UPLOAD_ID_PREFIX = "upload-"
        const val STORED_ATTEMPT_COUNT = 3
        const val MAX_ATTEMPTS = 3

        const val STORE_FILE_NAME = "upload-queue.txt"
        const val READ_FAILURE_MESSAGE = "store read failed"
        const val WRITE_FAILURE_MESSAGE = "store write failed"
    }
}
