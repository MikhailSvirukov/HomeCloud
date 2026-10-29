package dev.homecloud.android.transfer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import dev.homecloud.android.R

/**
 * Builds the ongoing notification that makes a long upload transfer visible while it runs
 * (`CON-AND-004`). It carries no file names, paths or credentials.
 */
internal object UploadTransferNotification {
    /** Id of the channel that carries transfer notifications. */
    const val CHANNEL_ID = "upload-transfer"

    /** Id of the ongoing transfer notification. */
    const val NOTIFICATION_ID = 4001

    /**
     * Creates the transfer channel once. Calling it again leaves the existing channel and its
     * user-visible settings untouched.
     */
    fun ensureChannel(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.upload_transfer_channel_name),
                NotificationManager.IMPORTANCE_LOW,
            )
        channel.description = context.getString(R.string.upload_transfer_channel_description)
        manager.createNotificationChannel(channel)
    }

    /**
     * Builds the ongoing transfer notification on [CHANNEL_ID]: the app name as title, a generic
     * transfer-in-progress text and a single alert. It contains no file names, paths or credentials.
     */
    fun build(context: Context): Notification =
        NotificationCompat
            .Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(context.getString(R.string.upload_transfer_notification_title))
            .setContentText(context.getString(R.string.upload_transfer_notification_text))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()
}
