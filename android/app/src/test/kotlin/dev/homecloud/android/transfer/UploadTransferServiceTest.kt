package dev.homecloud.android.transfer

import android.app.NotificationManager
import android.app.Service
import android.content.ComponentName
import android.content.pm.ServiceInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Drives the foreground-transfer service of `CON-AND-004` and `CON-AND-005`: the start command
 * posts the ongoing transfer notification under its fixed id and asks the platform not to restart
 * the service, and the merged manifest declares the service as a `dataSync` foreground service.
 *
 * `ServiceController.startCommand` discards the command result, so the lifecycle is created through
 * Robolectric and the command is invoked on the created service to observe the returned value.
 * Robolectric's shadow service records the posted foreground notification. The API 35 and API 28
 * scenarios cover both `startForeground` overloads, and a separate scenario observes the channel
 * registered on create.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [UploadTransferServiceTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class UploadTransferServiceTest {
    @Test
    fun `start command returns not sticky and posts the transfer notification in the foreground`() {
        assertStartCommandPostsTransferNotification()
    }

    @Test
    @Config(sdk = [MODERN_SDK_INT])
    fun `start command on api 35 returns not sticky and posts the transfer notification in the foreground`() {
        assertStartCommandPostsTransferNotification()
    }

    @Test
    @Config(sdk = [LEGACY_SDK_INT])
    fun `start command on api 28 returns not sticky and posts the transfer notification in the foreground`() {
        assertStartCommandPostsTransferNotification()
    }

    @Test
    fun `create registers the upload transfer notification channel`() {
        Robolectric.buildService(UploadTransferService::class.java).create().get()

        val notificationManager =
            RuntimeEnvironment.getApplication().getSystemService(NotificationManager::class.java)

        assertNotNull(
            "onCreate must register the channel before the start command posts a notification",
            notificationManager.getNotificationChannel(UploadTransferNotification.CHANNEL_ID),
        )
    }

    @Test
    fun `manifest declares the service with the data sync foreground type`() {
        val application = RuntimeEnvironment.getApplication()
        val serviceInfo =
            application.packageManager.getServiceInfo(
                ComponentName(application, UploadTransferService::class.java),
                0,
            )

        assertEquals(
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            serviceInfo.foregroundServiceType,
        )
    }

    private fun assertStartCommandPostsTransferNotification() {
        val service =
            Robolectric
                .buildService(UploadTransferService::class.java)
                .create()
                .get()

        val result = service.onStartCommand(null, 0, 0)

        assertEquals(Service.START_NOT_STICKY, result)
        val shadowService = shadowOf(service)
        assertEquals(
            UploadTransferNotification.NOTIFICATION_ID,
            shadowService.lastForegroundNotificationId,
        )
        assertNotNull(
            "the service must post a foreground notification",
            shadowService.lastForegroundNotification,
        )
    }

    private companion object {
        const val SDK_INT = 29
        const val MODERN_SDK_INT = 35
        const val LEGACY_SDK_INT = 28
    }
}
