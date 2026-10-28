package dev.homecloud.android.backup

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.work.Constraints
import androidx.work.NetworkType
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.testing.WorkManagerTestInitHelper
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.util.concurrent.TimeUnit

/**
 * Inspects the work [BackupWorkScheduler] enqueues through a test [WorkManager], so the schedule
 * names, constraints, period, and idempotence are read back from [WorkInfo] instead of from the
 * requests the scheduler builds.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [BackupWorkSchedulerTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class BackupWorkSchedulerTest {
    private lateinit var workManager: WorkManager
    private lateinit var scheduler: BackupWorkScheduler

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        WorkManagerTestInitHelper.initializeTestWorkManager(context)
        workManager = WorkManager.getInstance(context)
        scheduler = BackupWorkScheduler(workManager)
    }

    @Test
    fun `schedulePeriodic enqueues a six hour cycle that needs neither charging nor a network`() {
        scheduler.schedulePeriodic()

        val workInfo = singleWorkInfo(BackupWorkScheduler.PERIODIC_WORK_NAME)

        assertEquals(WorkInfo.State.ENQUEUED, workInfo.state)
        assertCycleConstraints(workInfo.constraints)
        val periodicity =
            requireNotNull(workInfo.periodicityInfo) {
                "periodic work must report a periodicity"
            }
        assertEquals(
            TimeUnit.HOURS.toMillis(BackupWorkScheduler.CYCLE_INTERVAL_HOURS),
            periodicity.repeatIntervalMillis,
        )
    }

    @Test
    fun `schedulePeriodic twice keeps the already enqueued cycle`() {
        scheduler.schedulePeriodic()
        val firstId = singleWorkInfo(BackupWorkScheduler.PERIODIC_WORK_NAME).id

        scheduler.schedulePeriodic()

        val workInfos = workInfosFor(BackupWorkScheduler.PERIODIC_WORK_NAME)
        assertEquals(SINGLE_WORK, workInfos.size)
        assertEquals(firstId, workInfos.single().id)
    }

    @Test
    fun `runNow enqueues a one-time cycle that needs neither charging nor a network`() {
        scheduler.runNow()

        val workInfo = singleWorkInfo(BackupWorkScheduler.ONE_TIME_WORK_NAME)

        assertEquals(WorkInfo.State.ENQUEUED, workInfo.state)
        assertCycleConstraints(workInfo.constraints)
        assertNull(workInfo.periodicityInfo)
    }

    @Test
    fun `runNow twice keeps the already pending cycle`() {
        scheduler.runNow()
        val firstId = singleWorkInfo(BackupWorkScheduler.ONE_TIME_WORK_NAME).id

        scheduler.runNow()

        val workInfos = workInfosFor(BackupWorkScheduler.ONE_TIME_WORK_NAME)
        assertEquals(SINGLE_WORK, workInfos.size)
        assertEquals(firstId, workInfos.single().id)
    }

    private fun singleWorkInfo(name: String): WorkInfo = workInfosFor(name).single()

    private fun workInfosFor(name: String): List<WorkInfo> = workManager.getWorkInfosForUniqueWork(name).get()

    private fun assertCycleConstraints(constraints: Constraints) {
        assertFalse(constraints.requiresCharging())
        assertFalse(constraints.requiresBatteryNotLow())
        assertEquals(NetworkType.NOT_REQUIRED, constraints.requiredNetworkType)
    }

    private companion object {
        const val SDK_INT = 30
        const val SINGLE_WORK = 1
    }
}
