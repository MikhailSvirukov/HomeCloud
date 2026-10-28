package dev.homecloud.android.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins when `FR-AND-035` asks for the runtime notification permission: on API 33 and above only
 * while it is denied.
 */
class NotificationPermissionPolicyTest {
    @Test
    fun `should request on api 33 and above when the notification permission is denied`() {
        PERMISSION_SDK_LEVELS.forEach { sdkInt ->
            assertTrue(
                "sdk $sdkInt should request the notification permission",
                NotificationPermissionPolicy.shouldRequest(sdkInt, granted = false),
            )
        }
    }

    @Test
    fun `should not request when the notification permission is already granted`() {
        PERMISSION_SDK_LEVELS.forEach { sdkInt ->
            assertFalse(
                "sdk $sdkInt should not request a granted notification permission",
                NotificationPermissionPolicy.shouldRequest(sdkInt, granted = true),
            )
        }
    }

    @Test
    fun `should not request below api 33`() {
        LEGACY_SDK_LEVELS.forEach { sdkInt ->
            assertFalse(
                "sdk $sdkInt predates the notification permission",
                NotificationPermissionPolicy.shouldRequest(sdkInt, granted = false),
            )
        }
    }

    private companion object {
        const val LEGACY_SDK_INT = 32
        val LEGACY_SDK_LEVELS = listOf(26, LEGACY_SDK_INT)
        val PERMISSION_SDK_LEVELS =
            listOf(NotificationPermissionPolicy.NOTIFICATION_PERMISSION_SDK_INT, 34, 35, 36)
    }
}
