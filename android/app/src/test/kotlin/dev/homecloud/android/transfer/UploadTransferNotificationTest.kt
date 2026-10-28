package dev.homecloud.android.transfer

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import dev.homecloud.android.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Builds the foreground-transfer notification of `CON-AND-004` against the real notification
 * service, so the channel registration and the user-visible content are covered without posting a
 * notification or requesting the runtime permission.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [UploadTransferNotificationTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class UploadTransferNotificationTest {
    private val context: Context = ApplicationProvider.getApplicationContext()

    @Test
    fun `ensure channel creates the low importance transfer channel with its resource texts`() {
        UploadTransferNotification.ensureChannel(context)

        val channel =
            requireNotNull(notificationManager().getNotificationChannel(UploadTransferNotification.CHANNEL_ID)) {
                "the transfer channel must be registered"
            }
        assertEquals(NotificationManager.IMPORTANCE_LOW, channel.importance)
        assertEquals(context.getString(R.string.upload_transfer_channel_name), channel.name)
        assertEquals(
            context.getString(R.string.upload_transfer_channel_description),
            channel.description.toString(),
        )
    }

    @Test
    fun `build uses the transfer channel and ongoing single-alert content`() {
        val notification = UploadTransferNotification.build(context)

        assertEquals(UploadTransferNotification.CHANNEL_ID, notification.channelId)
        assertTrue(
            "the transfer notification must stay ongoing",
            notification.flags and Notification.FLAG_ONGOING_EVENT != 0,
        )
        assertTrue(
            "the transfer notification must alert only once",
            notification.flags and Notification.FLAG_ONLY_ALERT_ONCE != 0,
        )
        assertEquals(
            context.getString(R.string.upload_transfer_notification_title),
            notification.extras.getCharSequence(Notification.EXTRA_TITLE)?.toString(),
        )
        assertEquals(
            context.getString(R.string.upload_transfer_notification_text),
            notification.extras.getCharSequence(Notification.EXTRA_TEXT)?.toString(),
        )
        val smallIcon =
            requireNotNull(notification.smallIcon) {
                "the transfer notification must carry a small icon"
            }
        assertNotEquals(0, smallIcon.resId)
    }

    private fun notificationManager(): NotificationManager = context.getSystemService(NotificationManager::class.java)

    private companion object {
        const val SDK_INT = 33
    }
}
