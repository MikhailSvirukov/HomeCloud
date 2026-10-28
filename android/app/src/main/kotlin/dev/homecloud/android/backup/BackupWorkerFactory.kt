package dev.homecloud.android.backup

import android.content.Context
import androidx.work.ListenableWorker
import androidx.work.WorkerFactory
import androidx.work.WorkerParameters

/**
 * Builds [BackupWorker] instances for WorkManager with a runner supplied per worker.
 *
 * Only the backup worker class is handled. Any other worker class yields no worker, so
 * WorkManager's default factory keeps handling the rest and the app supports future workers
 * without changing this factory. A fresh runner per worker keeps no cycle state shared between
 * executions (`CON-AND-001 / 1`).
 */
internal class BackupWorkerFactory(
    private val runnerProvider: () -> BackupCycleRunner,
) : WorkerFactory() {
    override fun createWorker(
        appContext: Context,
        workerClassName: String,
        workerParameters: WorkerParameters,
    ): ListenableWorker? =
        if (workerClassName == BackupWorker::class.java.name) {
            BackupWorker(appContext, workerParameters, runnerProvider())
        } else {
            null
        }
}
