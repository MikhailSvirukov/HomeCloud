package dev.homecloud.android.backup

import android.content.Context
import androidx.work.ListenableWorker
import androidx.work.Worker
import androidx.work.WorkerParameters

/**
 * Runs exactly one backup cycle per work execution (`CON-AND-001 / 1`).
 *
 * A completed cycle reports success. A cycle that throws reports retry instead, so WorkManager
 * runs the work again later rather than dropping it. The queue is persisted independently of this
 * worker, so a retry or a worker stop keeps every pending upload available (`CON-AND-002 / 1`,
 * `CON-AND-003 / 1`, `NFR-REL-001 / 1`). The cycle logs its own failure, so this worker adds no
 * logging and never exposes media content, file names, or credentials.
 */
internal class BackupWorker(
    context: Context,
    params: WorkerParameters,
    private val runner: BackupCycleRunner,
) : Worker(context, params) {
    override fun doWork(): ListenableWorker.Result =
        try {
            runner.runOnce()
            ListenableWorker.Result.success()
        } catch (_: Exception) {
            ListenableWorker.Result.retry()
        }
}
