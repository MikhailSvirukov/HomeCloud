package dev.homecloud.android.backup

import android.content.Context
import android.content.pm.ServiceInfo
import androidx.test.core.app.ApplicationProvider
import androidx.work.ForegroundInfo
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueStore
import dev.homecloud.android.testsupport.FakeMediaVersionScanner
import dev.homecloud.android.transfer.UploadTransferNotification
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Runs [BackupWorker] through WorkManager's test worker builder, so the worker's result mapping
 * and foreground publication are exercised without enqueueing real work.
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

        val result = runBlocking { worker.doWork() }

        assertEquals(ListenableWorker.Result.success(), result)
        assertEquals(SINGLE_CYCLE, store.readCount)
        assertEquals(SINGLE_CYCLE, store.writeCount)
        assertEquals(SINGLE_CYCLE, scanner.scanCount)
    }

    @Test
    fun `a failing cycle reports retry instead of propagating the failure`() {
        val store = RecordingUploadQueueStore(readFailure = IllegalStateException(CYCLE_FAILURE_MESSAGE))
        val worker = workerFor(store, FakeMediaVersionScanner())

        val result = runBlocking { worker.doWork() }

        assertEquals(ListenableWorker.Result.retry(), result)
        assertEquals(SINGLE_CYCLE, store.readCount)
    }

    @Test
    fun `the foreground publisher receives the transfer notification with the data sync type`() {
        var published: ForegroundInfo? = null
        val worker =
            workerFor(
                RecordingUploadQueueStore(),
                FakeMediaVersionScanner(),
                foregroundPublisher = { info -> published = info },
            )

        runBlocking { worker.doWork() }

        val info = requireNotNull(published) { "the worker must publish foreground work" }
        assertEquals(UploadTransferNotification.NOTIFICATION_ID, info.notificationId)
        assertEquals(UploadTransferNotification.CHANNEL_ID, info.notification.channelId)
        assertEquals(
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            info.foregroundServiceType,
        )
    }

    @Test
    fun `the foreground notification is published once before the cycle reads the queue`() {
        val events = mutableListOf<String>()
        val store = RecordingUploadQueueStore(onRead = { events += CYCLE_EVENT })
        val worker =
            workerFor(
                store,
                FakeMediaVersionScanner(),
                foregroundPublisher = { events += PUBLISH_EVENT },
            )

        runBlocking { worker.doWork() }

        assertEquals(listOf(PUBLISH_EVENT, CYCLE_EVENT), events)
    }

    private fun workerFor(
        store: UploadQueueStore,
        scanner: FakeMediaVersionScanner,
        foregroundPublisher: (suspend (ForegroundInfo) -> Unit) = {},
    ): BackupWorker {
        val planner =
            MediaBackupPlanner(
                scanner,
                InMemoryUploadQueue(MAX_ATTEMPTS),
                maxQueueSize = MAX_QUEUE_SIZE,
            )
        return TestListenableWorkerBuilder<BackupWorker>(context())
            .setWorkerFactory(
                BackupWorkerFactory(
                    runnerProvider = { BackupCycleRunner(planner, store) },
                    foregroundPublisher = foregroundPublisher,
                ),
            ).build()
    }

    private fun context(): Context = ApplicationProvider.getApplicationContext<Context>()

    private class RecordingUploadQueueStore(
        private val readFailure: Exception? = null,
        private val onRead: () -> Unit = {},
    ) : UploadQueueStore {
        var readCount: Int = 0
            private set

        var writeCount: Int = 0
            private set

        override fun read(): List<UploadQueueEntry> {
            onRead()
            readCount += 1
            val failure = readFailure
            return if (failure == null) emptyList() else throw failure
        }

        override fun write(entries: List<UploadQueueEntry>) {
            writeCount += 1
        }
    }

    private companion object {
        const val SDK_INT = 35
        const val MAX_ATTEMPTS = 3
        const val MAX_QUEUE_SIZE = 100
        const val SINGLE_CYCLE = 1
        const val CYCLE_FAILURE_MESSAGE = "cycle could not read the queue"
        const val PUBLISH_EVENT = "publish foreground"
        const val CYCLE_EVENT = "read queue"
    }
}
