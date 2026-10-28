package dev.homecloud.android.notifications

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the runtime notification decision of `FR-AND-035` and `FR-AND-036`: the permission is asked
 * for on API 33 and above only while it is denied, and a denied permission never turns into an
 * app-level block.
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

    @Test
    fun `cannot show at api 33 when the notification permission is denied`() {
        assertFalse(
            NotificationPermissionPolicy.canShow(
                NotificationPermissionPolicy.NOTIFICATION_PERMISSION_SDK_INT,
                granted = false,
            ),
        )
    }

    @Test
    fun `can show at api 33 when the notification permission is granted`() {
        assertTrue(
            NotificationPermissionPolicy.canShow(
                NotificationPermissionPolicy.NOTIFICATION_PERMISSION_SDK_INT,
                granted = true,
            ),
        )
    }

    @Test
    fun `can show below api 33 without the notification permission`() {
        assertTrue(
            NotificationPermissionPolicy.canShow(
                LEGACY_SDK_INT,
                granted = false,
            ),
        )
    }

    private companion object {
        const val LEGACY_SDK_INT = 32
        val LEGACY_SDK_LEVELS = listOf(26, LEGACY_SDK_INT)
        val PERMISSION_SDK_LEVELS =
            listOf(NotificationPermissionPolicy.NOTIFICATION_PERMISSION_SDK_INT, 34, 35, 36)
    }
}
