package dev.homecloud.android.backup

import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequest
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequest
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

/**
 * Schedules the backup cycle through WorkManager (`CON-AND-001 / 1`).
 *
 * The periodic request repeats every six hours. The interval is well above the platform minimum
 * because each cycle re-hashes the whole media library, so a long cadence keeps repeated work
 * bounded while still discovering new media (`FR-AND-002 / 1`). The one-time request exists for a
 * cycle requested from the app.
 *
 * Both requests carry constraints that never require charging or a battery above the system's low
 * threshold, and no network requirement, because scanning and queueing are local work
 * (`CON-AND-006 / 1`).
 *
 * Scheduling is idempotent: the periodic request keeps the already-enqueued cycle and the one-time
 * request keeps an already-pending cycle, so repeated calls never create duplicates.
 */
internal class BackupWorkScheduler(
    private val workManager: WorkManager,
) {
    /**
     * Ensures the repeating backup cycle is enqueued, leaving an existing scheduled cycle in place.
     */
    fun schedulePeriodic() {
        workManager.enqueueUniquePeriodicWork(
            PERIODIC_WORK_NAME,
            ExistingPeriodicWorkPolicy.KEEP,
            periodicRequest(),
        )
    }

    /**
     * Requests one immediate backup cycle, leaving an already-pending cycle in place.
     */
    fun runNow() {
        workManager.enqueueUniqueWork(
            ONE_TIME_WORK_NAME,
            ExistingWorkPolicy.KEEP,
            oneTimeRequest(),
        )
    }

    internal fun periodicRequest(): PeriodicWorkRequest =
        PeriodicWorkRequestBuilder<BackupWorker>(CYCLE_INTERVAL_HOURS, TimeUnit.HOURS)
            .setConstraints(cycleConstraints)
            .build()

    internal fun oneTimeRequest(): OneTimeWorkRequest =
        OneTimeWorkRequestBuilder<BackupWorker>()
            .setConstraints(cycleConstraints)
            .build()

    companion object {
        /** Unique name of the repeating cycle, so scheduling it again replaces nothing. */
        const val PERIODIC_WORK_NAME = "backup-cycle-periodic"

        /** Unique name of an on-demand cycle, so repeated requests collapse into one. */
        const val ONE_TIME_WORK_NAME = "backup-cycle-now"

        /** Repeating interval in hours, chosen to bound the cost of re-hashing the library. */
        const val CYCLE_INTERVAL_HOURS = 6L

        /**
         * Cycle constraints that keep the backup runnable on battery power and without a network
         * route, since scanning and queueing are local (`CON-AND-006 / 1`).
         */
        val cycleConstraints: Constraints =
            Constraints
                .Builder()
                .setRequiresCharging(false)
                .setRequiresBatteryNotLow(false)
                .build()
    }
}
