package dev.homecloud.android.backup

import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.ListenableWorker
import androidx.work.WorkerParameters
import dev.homecloud.android.transfer.UploadTransferNotification
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Runs exactly one backup cycle per work execution as foreground work (`CON-AND-001 / 1`,
 * `CON-AND-004 / 1`).
 *
 * Before the cycle, the worker publishes an ongoing foreground notification with the `dataSync`
 * service type so a long transfer stays visible to the user and the system does not stop it
 * (`CON-AND-004 / 1`, `CON-AND-005 / 1`). The cycle then runs on a background dispatcher. The
 * worker never starts [dev.homecloud.android.transfer.UploadTransferService]; the service and the
 * worker share the same notification channel and id, so at most one transfer notification is
 * visible at a time (`CON-AND-004 / 1`).
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
    private val foregroundPublisher: (suspend (ForegroundInfo) -> Unit)? = null,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): ListenableWorker.Result =
        try {
            val info = foregroundInfo()
            if (foregroundPublisher != null) {
                foregroundPublisher(info)
            } else {
                setForeground(info)
            }
            withContext(Dispatchers.IO) {
                runner.runOnce()
            }
            ListenableWorker.Result.success()
        } catch (_: Exception) {
            ListenableWorker.Result.retry()
        }

    /**
     * Builds the ongoing foreground notification on the shared transfer channel and marks it with
     * the `dataSync` service type on API 29 and above (`CON-AND-004 / 1`, `CON-AND-005 / 1`).
     */
    internal fun foregroundInfo(): ForegroundInfo {
        UploadTransferNotification.ensureChannel(applicationContext)
        val notification = UploadTransferNotification.build(applicationContext)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(
                UploadTransferNotification.NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            ForegroundInfo(UploadTransferNotification.NOTIFICATION_ID, notification)
        }
    }
}
