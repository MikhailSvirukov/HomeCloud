package dev.homecloud.android.backup

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueStore
import dev.homecloud.android.testsupport.FakeMediaVersionScanner
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Runs [BackupWorker] through WorkManager's test worker builder, so the worker's result mapping is
 * exercised without enqueueing real work.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [BackupWorkerTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class BackupWorkerTest {
    @Test
    fun `a completed cycle reports success and runs the runner once`() {
        val store = RecordingUploadQueueStore()
        val scanner = FakeMediaVersionScanner()
        val worker = workerFor(store, scanner)

        val result = worker.doWork()

        assertEquals(ListenableWorker.Result.success(), result)
        assertEquals(SINGLE_CYCLE, store.readCount)
        assertEquals(SINGLE_CYCLE, store.writeCount)
        assertEquals(SINGLE_CYCLE, scanner.scanCount)
    }

    @Test
    fun `a failing cycle reports retry instead of propagating the failure`() {
        val store = RecordingUploadQueueStore(readFailure = IllegalStateException(CYCLE_FAILURE_MESSAGE))
        val worker = workerFor(store, FakeMediaVersionScanner())

        val result = worker.doWork()

        assertEquals(ListenableWorker.Result.retry(), result)
        assertEquals(SINGLE_CYCLE, store.readCount)
    }

    private fun workerFor(
        store: UploadQueueStore,
        scanner: FakeMediaVersionScanner,
    ): BackupWorker {
        val planner =
            MediaBackupPlanner(
                scanner,
                InMemoryUploadQueue(MAX_ATTEMPTS),
                maxQueueSize = MAX_QUEUE_SIZE,
            )
        return TestListenableWorkerBuilder<BackupWorker>(context())
            .setWorkerFactory(BackupWorkerFactory { BackupCycleRunner(planner, store) })
            .build()
    }

    private fun context(): Context = ApplicationProvider.getApplicationContext<Context>()

    private class RecordingUploadQueueStore(
        private val readFailure: Exception? = null,
    ) : UploadQueueStore {
        var readCount: Int = 0
            private set

        var writeCount: Int = 0
            private set

        override fun read(): List<UploadQueueEntry> {
            readCount += 1
            val failure = readFailure
            return if (failure == null) emptyList() else throw failure
        }

        override fun write(entries: List<UploadQueueEntry>) {
            writeCount += 1
        }
    }

    private companion object {
        const val SDK_INT = 30
        const val MAX_ATTEMPTS = 3
        const val MAX_QUEUE_SIZE = 100
        const val SINGLE_CYCLE = 1
        const val CYCLE_FAILURE_MESSAGE = "cycle could not read the queue"
    }
}
