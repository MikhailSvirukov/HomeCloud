package dev.homecloud.android.transfer

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Foreground service that keeps a long upload transfer visible and alive (`CON-AND-004`,
 * `CON-AND-005`). It only owns the notification and its lifetime; starting and stopping the
 * transfer around it belongs to the caller.
 */
internal class UploadTransferService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        UploadTransferNotification.ensureChannel(this)
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        val notification = UploadTransferNotification.build(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                UploadTransferNotification.NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            startForeground(UploadTransferNotification.NOTIFICATION_ID, notification)
        }
        return START_NOT_STICKY
    }
}
