package dev.homecloud.android.backup

import dev.homecloud.android.log.BackupCycleLog
import dev.homecloud.android.log.NoOpBackupCycleLog
import dev.homecloud.android.media.MediaKind
import dev.homecloud.android.media.MediaLibraryUnreadableException
import dev.homecloud.android.media.MediaVersionCandidate
import dev.homecloud.android.media.MediaVersionScanner
import dev.homecloud.android.queue.FileUploadQueueStore
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.queue.UploadQueueStore
import dev.homecloud.android.queue.UploadQueueStoreException
import dev.homecloud.android.status.CycleOutcome
import dev.homecloud.android.status.LastCycleRecord
import dev.homecloud.android.status.LastCycleStore
import dev.homecloud.android.status.LastErrorCategory
import dev.homecloud.android.testsupport.FakeMediaVersionScanner
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException

class BackupCycleRunnerTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun `runOnce over an empty store enqueues every discovered image and video candidate`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val runner = runnerFor(store = FileUploadQueueStore(storeFile()), queue = queue)

        val result = runner.runOnce()

        assertEquals(
            PlanResult(
                discoveredCandidates = CANDIDATE_COUNT,
                newlyEnqueued = CANDIDATE_COUNT,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
        assertEquals(
            listOf(IMAGE_ID, VIDEO_ID),
            queue.entries().map { entry -> entry.sourceId },
        )
        assertEquals(
            CANDIDATE_COUNT,
            queue
                .entries()
                .map { entry -> entry.uploadId }
                .distinct()
                .size,
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
                scanner = FakeMediaVersionScanner(listOf(MediaTestData.candidate(id = CANDIDATE_ID, kind = MediaKind.IMAGE))),
            )

        val result = runner.runOnce()

        assertEquals(
            PlanResult(
                discoveredCandidates = SINGLE_CANDIDATE_COUNT,
                newlyEnqueued = NO_NEW_ENQUEUES,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
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

        assertEquals(
            PlanResult(CANDIDATE_COUNT, CANDIDATE_COUNT, NO_CAPACITY_REJECTIONS),
            firstResult,
        )
        assertEquals(NO_NEW_ENQUEUES, secondResult.newlyEnqueued)
        assertEquals(CANDIDATE_COUNT, secondResult.discoveredCandidates)
        assertEquals(firstQueue.entries(), secondQueue.entries())
        assertEquals(CANDIDATE_COUNT, store.read().size)
    }

    @Test
    fun `runOnce with no discovered candidates reports an empty plan and stores an empty snapshot`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val store = FileUploadQueueStore(storeFile())
        val runner = runnerFor(store = store, queue = queue, scanner = FakeMediaVersionScanner(emptyList()))

        val result = runner.runOnce()

        assertEquals(
            PlanResult(
                discoveredCandidates = 0,
                newlyEnqueued = 0,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
        assertTrue(store.read().isEmpty())
    }

    @Test
    fun `runOnce propagates a store read failure and writes nothing`() {
        val failure = UploadQueueStoreException(READ_FAILURE_MESSAGE)
        val store = FakeUploadQueueStore(readFailure = failure)
        val log = RecordingBackupCycleLog()
        val runner = runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS), log = log)

        val thrown = assertThrows(UploadQueueStoreException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
        assertEquals(0, store.writeCount)
        assertEquals(SINGLE_FAILURE_EVENT, log.failureCount)
        assertTrue(log.plannedResults.isEmpty())
    }

    @Test
    fun `runOnce propagates a store write failure`() {
        val failure = UploadQueueStoreException(WRITE_FAILURE_MESSAGE)
        val store = FakeUploadQueueStore(writeFailure = failure)
        val runner = runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS))

        val thrown = assertThrows(UploadQueueStoreException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
    }

    @Test
    fun `runOnce records one planned event carrying the counts of a completed cycle`() {
        val log = RecordingBackupCycleLog()
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                log = log,
            )

        val result = runner.runOnce()

        assertEquals(
            PlanResult(
                discoveredCandidates = CANDIDATE_COUNT,
                newlyEnqueued = CANDIDATE_COUNT,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
        assertEquals(listOf(result), log.plannedResults)
        assertEquals(NO_FAILURE_EVENTS, log.failureCount)
    }

    @Test
    fun `runOnce reports a capacity rejection to the log unchanged`() {
        val log = RecordingBackupCycleLog()
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                log = log,
                maxQueueSize = CAPACITY,
            )

        val result = runner.runOnce()

        val expected =
            PlanResult(
                discoveredCandidates = CANDIDATE_COUNT,
                newlyEnqueued = CAPACITY,
                rejectedByCapacity = SINGLE_REJECTION,
            )
        assertEquals(expected, result)
        assertEquals(listOf(expected), log.plannedResults)
    }

    @Test
    fun `runOnce over the same store twice records each cycle's own newly enqueued count`() {
        val store = FileUploadQueueStore(storeFile())
        val firstLog = RecordingBackupCycleLog()
        val secondLog = RecordingBackupCycleLog()

        runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS), log = firstLog).runOnce()
        runnerFor(store = store, queue = InMemoryUploadQueue(MAX_ATTEMPTS), log = secondLog).runOnce()

        assertEquals(
            listOf(PlanResult(CANDIDATE_COUNT, CANDIDATE_COUNT, NO_CAPACITY_REJECTIONS)),
            firstLog.plannedResults,
        )
        assertEquals(
            listOf(PlanResult(CANDIDATE_COUNT, NO_NEW_ENQUEUES, NO_CAPACITY_REJECTIONS)),
            secondLog.plannedResults,
        )
    }

    @Test
    fun `runOnce records the completion time and success of a completed cycle`() {
        val lastCycleStore = RecordingLastCycleStore()
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                lastCycleStore = lastCycleStore,
                clock = { LAST_CYCLE_COMPLETED_AT_MILLIS },
            )

        runner.runOnce()

        assertEquals(
            listOf(
                LastCycleRecord(
                    completedAtMillis = LAST_CYCLE_COMPLETED_AT_MILLIS,
                    outcome = CycleOutcome.SUCCESS,
                    error = null,
                ),
            ),
            lastCycleStore.records,
        )
    }

    @Test
    fun `runOnce records the queue category and rethrows a store read failure`() {
        val failure = IOException(QUEUE_FAILURE_MESSAGE)
        val lastCycleStore = RecordingLastCycleStore()
        val runner =
            runnerFor(
                store = FakeUploadQueueStore(readFailure = failure),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                lastCycleStore = lastCycleStore,
                clock = { LAST_CYCLE_COMPLETED_AT_MILLIS },
            )

        val thrown = assertThrows(IOException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
        assertEquals(
            listOf(
                LastCycleRecord(
                    completedAtMillis = LAST_CYCLE_COMPLETED_AT_MILLIS,
                    outcome = CycleOutcome.FAILURE,
                    error = LastErrorCategory.QUEUE_UNAVAILABLE,
                ),
            ),
            lastCycleStore.records,
        )
    }

    @Test
    fun `runOnce records the media category and rethrows a media library failure`() {
        val failure = MediaLibraryUnreadableException(MEDIA_FAILURE_MESSAGE)
        val lastCycleStore = RecordingLastCycleStore()
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                scanner = MediaVersionScanner { throw failure },
                lastCycleStore = lastCycleStore,
                clock = { LAST_CYCLE_COMPLETED_AT_MILLIS },
            )

        val thrown = assertThrows(MediaLibraryUnreadableException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
        assertEquals(
            listOf(
                LastCycleRecord(
                    completedAtMillis = LAST_CYCLE_COMPLETED_AT_MILLIS,
                    outcome = CycleOutcome.FAILURE,
                    error = LastErrorCategory.MEDIA_LIBRARY_UNREADABLE,
                ),
            ),
            lastCycleStore.records,
        )
    }

    @Test
    fun `runOnce records the internal category and rethrows an unexpected failure`() {
        val failure = IllegalStateException(INTERNAL_FAILURE_MESSAGE)
        val lastCycleStore = RecordingLastCycleStore()
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                scanner = MediaVersionScanner { throw failure },
                lastCycleStore = lastCycleStore,
                clock = { LAST_CYCLE_COMPLETED_AT_MILLIS },
            )

        val thrown = assertThrows(IllegalStateException::class.java) { runner.runOnce() }

        assertSame(failure, thrown)
        assertEquals(
            listOf(
                LastCycleRecord(
                    completedAtMillis = LAST_CYCLE_COMPLETED_AT_MILLIS,
                    outcome = CycleOutcome.FAILURE,
                    error = LastErrorCategory.INTERNAL,
                ),
            ),
            lastCycleStore.records,
        )
    }

    @Test
    fun `runOnce without a last cycle store writes no cycle record`() {
        val lastCycleFile = File(temporaryFolder.newFolder(), LAST_CYCLE_FILE_NAME)
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                lastCycleStore = null,
            )

        runner.runOnce()

        assertFalse(lastCycleFile.exists())
    }

    @Test
    fun `runOnce with a record store that fails to write still returns the planned result`() {
        val runner =
            runnerFor(
                store = FileUploadQueueStore(storeFile()),
                queue = InMemoryUploadQueue(MAX_ATTEMPTS),
                lastCycleStore = FailingLastCycleStore(),
            )

        val result = runner.runOnce()

        assertEquals(
            PlanResult(
                discoveredCandidates = CANDIDATE_COUNT,
                newlyEnqueued = CANDIDATE_COUNT,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
    }

    private fun runnerFor(
        store: UploadQueueStore,
        queue: InMemoryUploadQueue,
        scanner: MediaVersionScanner = FakeMediaVersionScanner(imageAndVideoCandidates()),
        log: BackupCycleLog = NoOpBackupCycleLog,
        maxQueueSize: Int = NON_BINDING_CAPACITY,
        lastCycleStore: LastCycleStore? = null,
        clock: () -> Long = System::currentTimeMillis,
    ): BackupCycleRunner {
        val planner = MediaBackupPlanner(scanner, queue, maxQueueSize)
        return BackupCycleRunner(
            planner = planner,
            store = store,
            log = log,
            lastCycleStore = lastCycleStore,
            clock = clock,
        )
    }

    private fun imageAndVideoCandidates(): List<MediaVersionCandidate> =
        listOf(
            MediaTestData.candidate(id = IMAGE_ID, kind = MediaKind.IMAGE),
            MediaTestData.candidate(id = VIDEO_ID, kind = MediaKind.VIDEO),
        )

    private fun entry(
        uploadId: String,
        state: UploadQueueState,
        attemptCount: Int = 0,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = CANDIDATE_ID,
            identity = MediaTestData.versionIdentity(sourceId = CANDIDATE_ID),
            sizeBytes = MediaTestData.SIZE_BYTES,
            attemptCount = attemptCount,
            state = state,
        )

    private fun storeFile(): File = File(temporaryFolder.newFolder(), STORE_FILE_NAME)

    private class FakeUploadQueueStore(
        private val readFailure: Exception? = null,
        private val writeFailure: Exception? = null,
    ) : UploadQueueStore {
        var writeCount: Int = 0
            private set

        override fun read(): List<UploadQueueEntry> = readFailure?.let { failure -> throw failure } ?: emptyList()

        override fun write(entries: List<UploadQueueEntry>) {
            writeCount += 1
            writeFailure?.let { failure -> throw failure }
        }
    }

    private class RecordingLastCycleStore : LastCycleStore {
        private val recorded = mutableListOf<LastCycleRecord>()

        val records: List<LastCycleRecord>
            get() = recorded

        override fun read(): LastCycleRecord? = null

        override fun write(record: LastCycleRecord) {
            recorded += record
        }
    }

    private class FailingLastCycleStore : LastCycleStore {
        override fun read(): LastCycleRecord? = null

        override fun write(record: LastCycleRecord) = throw IOException(RECORD_WRITE_FAILURE_MESSAGE)
    }

    private class RecordingBackupCycleLog : BackupCycleLog {
        private val recordedPlannedResults = mutableListOf<PlanResult>()

        val plannedResults: List<PlanResult>
            get() = recordedPlannedResults

        var failureCount: Int = 0
            private set

        override fun cyclePlanned(
            discoveredCandidates: Int,
            newlyEnqueued: Int,
            rejectedByCapacity: Int,
        ) {
            recordedPlannedResults +=
                PlanResult(
                    discoveredCandidates = discoveredCandidates,
                    newlyEnqueued = newlyEnqueued,
                    rejectedByCapacity = rejectedByCapacity,
                )
        }

        override fun cycleFailed() {
            failureCount += 1
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

        const val LAST_CYCLE_FILE_NAME = "last-cycle"
        const val LAST_CYCLE_COMPLETED_AT_MILLIS = 1_700_000_000_000L
        const val QUEUE_FAILURE_MESSAGE = "queue read failed"
        const val MEDIA_FAILURE_MESSAGE = "media library could not be read"
        const val INTERNAL_FAILURE_MESSAGE = "unexpected cycle failure"
        const val RECORD_WRITE_FAILURE_MESSAGE = "record write failed"

        const val NON_BINDING_CAPACITY = 100
        const val CAPACITY = 1
        const val SINGLE_REJECTION = 1

        const val NO_NEW_ENQUEUES = 0
        const val NO_CAPACITY_REJECTIONS = 0
        const val NO_FAILURE_EVENTS = 0
        const val SINGLE_FAILURE_EVENT = 1
    }
}
