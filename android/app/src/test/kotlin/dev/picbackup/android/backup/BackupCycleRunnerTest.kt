package dev.picbackup.android.backup

import dev.picbackup.android.media.MediaDiscovery
import dev.picbackup.android.media.MediaItem
import dev.picbackup.android.media.MediaRowSource
import dev.picbackup.android.media.RawMediaEntry
import dev.picbackup.android.queue.FileUploadQueueStore
import dev.picbackup.android.queue.InMemoryUploadQueue
import dev.picbackup.android.queue.UploadQueueEntry
import dev.picbackup.android.queue.UploadQueueState
import dev.picbackup.android.queue.UploadQueueStore
import dev.picbackup.android.queue.UploadQueueStoreException
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
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

        assertEquals(queue.entries(), store.read())
        assertEquals(CANDIDATE_COUNT, store.read().size)
    }

    @Test
    fun `runOnce loads an existing snapshot before planning so a tracked candidate is not enqueued again`() {
        val store = FileUploadQueueStore(storeFile())
        val existing =
            entry(
                uploadId = "$UPLOAD_ID_PREFIX$CANDIDATE_ID",
                state = UploadQueueState.IN_FLIGHT,
                attemptCount = RESTORED_ATTEMPT_COUNT,
            )
        store.write(listOf(existing))
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val runner =
            runnerFor(
                store = store,
                queue = queue,
                rows = listOf(candidateRow(id = CANDIDATE_ID, mimeType = IMAGE_MIME_TYPE)),
            )

        val result = runner.runOnce()

        assertEquals(PlanResult(discoveredCandidates = SINGLE_CANDIDATE_COUNT, newlyEnqueued = 0), result)
        assertEquals(listOf(existing), queue.entries())
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
    fun `runOnce leaves a stored failed entry failed with its attempt count`() {
        val store = FileUploadQueueStore(storeFile())
        store.write(
            listOf(
                entry(
                    uploadId = "$UPLOAD_ID_PREFIX$CANDIDATE_ID",
                    state = UploadQueueState.FAILED,
                    attemptCount = MAX_ATTEMPTS,
                ),
            ),
        )
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val runner =
            runnerFor(
                store = store,
                queue = queue,
                rows = listOf(candidateRow(id = CANDIDATE_ID, mimeType = IMAGE_MIME_TYPE)),
            )

        runner.runOnce()

        val queued = queue.entries().single()
        assertEquals(UploadQueueState.FAILED, queued.state)
        assertEquals(MAX_ATTEMPTS, queued.attemptCount)

        val persisted = store.read().single()
        assertEquals(UploadQueueState.FAILED, persisted.state)
        assertEquals(MAX_ATTEMPTS, persisted.attemptCount)
    }

    @Test
    fun `runOnce propagates a store read failure and writes nothing`() {
        val store = ThrowingUploadQueueStore()
        val runner = runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS))

        val thrown = assertThrows(UploadQueueStoreException::class.java) { runner.runOnce() }

        assertSame(store.failure, thrown)
        assertEquals(0, store.writeCount)
    }

    private fun runnerFor(
        store: UploadQueueStore,
        queue: InMemoryUploadQueue,
        rows: List<RawMediaEntry> = imageAndVideoRows(),
    ): BackupCycleRunner {
        val planner =
            MediaBackupPlanner(MediaDiscovery(FixedMediaRowSource(rows)), FakeUploadIdFactory(), queue)
        return BackupCycleRunner(planner = planner, queue = queue, store = store)
    }

    private fun imageAndVideoRows(): List<RawMediaEntry> =
        listOf(
            candidateRow(id = IMAGE_ID, mimeType = IMAGE_MIME_TYPE),
            candidateRow(id = VIDEO_ID, mimeType = VIDEO_MIME_TYPE),
        )

    private fun candidateRow(id: Long, mimeType: String): RawMediaEntry =
        RawMediaEntry(
            id = id,
            uri = "$MEDIA_URI_PREFIX$id",
            displayName = "$DISPLAY_NAME_PREFIX$id",
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            mimeType = mimeType,
        )

    private fun entry(
        uploadId: String,
        state: UploadQueueState,
        attemptCount: Int = 0,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = CANDIDATE_ID,
            sizeBytes = SIZE_BYTES,
            dateModifiedEpochSeconds = DATE_MODIFIED_EPOCH_SECONDS,
            attemptCount = attemptCount,
            state = state,
        )

    private fun storeFile(): File = File(temporaryFolder.newFolder(), STORE_FILE_NAME)

    private class FixedMediaRowSource(private val rows: List<RawMediaEntry>) : MediaRowSource {

        override fun readAll(): List<RawMediaEntry> = rows
    }

    private class FakeUploadIdFactory(
        private val uploadIdsBySourceId: Map<Long, String> = emptyMap(),
    ) : UploadIdFactory {

        override fun uploadIdFor(item: MediaItem): String =
            uploadIdsBySourceId[item.id] ?: "$UPLOAD_ID_PREFIX${item.id}"
    }

    private class ThrowingUploadQueueStore : UploadQueueStore {

        val failure = UploadQueueStoreException(READ_FAILURE_MESSAGE)

        var writeCount: Int = 0
            private set

        override fun read(): List<UploadQueueEntry> = throw failure

        override fun write(entries: List<UploadQueueEntry>) {
            writeCount += 1
        }
    }

    private companion object {
        const val IMAGE_MIME_TYPE = "image/jpeg"
        const val VIDEO_MIME_TYPE = "video/mp4"

        const val MEDIA_URI_PREFIX = "content://media/external/file/"
        const val DISPLAY_NAME_PREFIX = "item-"
        const val SIZE_BYTES = 1_024L
        const val DATE_MODIFIED_EPOCH_SECONDS = 1_700_000_000L

        const val IMAGE_ID = 1L
        const val VIDEO_ID = 2L
        const val CANDIDATE_ID = 3L
        const val CANDIDATE_COUNT = 2
        const val SINGLE_CANDIDATE_COUNT = 1

        const val UPLOAD_ID_PREFIX = "upload-"
        const val RESTORED_ATTEMPT_COUNT = 2
        const val MAX_ATTEMPTS = 3

        const val STORE_FILE_NAME = "upload-queue.txt"
        const val READ_FAILURE_MESSAGE = "store read failed"
    }
}
