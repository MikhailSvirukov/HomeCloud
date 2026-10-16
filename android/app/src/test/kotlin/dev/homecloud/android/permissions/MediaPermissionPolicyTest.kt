package dev.homecloud.android.permissions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class MediaPermissionPolicyTest {

    @Test
    fun `sdk below 33 requires only read external storage`() {
        LEGACY_SDK_LEVELS.forEach { sdkInt ->
            assertEquals(
                "sdk $sdkInt",
                listOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
                MediaPermissionPolicy.requiredPermissions(sdkInt),
            )
        }
    }

    @Test
    fun `sdk 33 and above requires exactly the image and video read permissions`() {
        MODERN_SDK_LEVELS.forEach { sdkInt ->
            assertEquals(
                "sdk $sdkInt",
                listOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
                MediaPermissionPolicy.requiredPermissions(sdkInt),
            )
        }
    }

    @Test
    fun `returned permissions never contain audio location contacts camera or storage write`() {
        ALL_SDK_LEVELS.forEach { sdkInt ->
            val permissions = MediaPermissionPolicy.requiredPermissions(sdkInt)
            FORBIDDEN_PERMISSIONS.forEach { forbidden ->
                assertFalse(
                    "sdk $sdkInt unexpectedly requested $forbidden",
                    permissions.contains(forbidden),
                )
            }
        }
    }

    private companion object {
        val LEGACY_SDK_LEVELS = listOf(26, 32)
        val MODERN_SDK_LEVELS = listOf(33, 36)
        val ALL_SDK_LEVELS = LEGACY_SDK_LEVELS + MODERN_SDK_LEVELS
        val FORBIDDEN_PERMISSIONS = listOf(
            "android.permission.READ_MEDIA_AUDIO",
            "android.permission.ACCESS_FINE_LOCATION",
            "android.permission.ACCESS_COARSE_LOCATION",
            "android.permission.READ_CONTACTS",
            "android.permission.CAMERA",
            "android.permission.WRITE_EXTERNAL_STORAGE",
        )
    }
}
