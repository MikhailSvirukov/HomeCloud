package dev.homecloud.android

import android.content.ComponentName
import android.content.pm.PackageManager
import dev.homecloud.android.notifications.NotificationPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionPolicy
import dev.homecloud.android.transfer.UploadTransferService
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Reads the application's requested permissions from the package manager and asserts the merged
 * manifest declares exactly the v1 platform permission set.
 *
 * The API-32 scenario observes the legacy storage read, whose `android:maxSdkVersion` cap hides it
 * from the framework's request set on later levels. The post-cap scenario asserts the remaining
 * platform permissions from API 33, so removing the cap or raising it above 32 is detected too.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [AndroidManifestPermissionsTest.LEGACY_READ_MAX_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class AndroidManifestPermissionsTest {
    @Test
    fun `manifest declares every v1 platform permission including the capped legacy read`() {
        assertEquals(CAPPED_PLATFORM_PERMISSIONS, platformPermissions())
    }

    @Test
    @Config(sdk = [AndroidManifestPermissionsTest.POST_CAP_SDK_INT])
    fun `manifest omits the capped legacy read from the post-cap platform permissions`() {
        assertEquals(POST_CAP_PLATFORM_PERMISSIONS, platformPermissions())
    }

    @Test
    fun `manifest declares the upload transfer service without exporting it`() {
        val application = RuntimeEnvironment.getApplication()
        val serviceInfo =
            application.packageManager.getServiceInfo(
                ComponentName(application, UploadTransferService::class.java),
                0,
            )

        assertFalse(serviceInfo.exported)
    }

    private fun platformPermissions(): Set<String> {
        val application = RuntimeEnvironment.getApplication()
        val packageInfo =
            application.packageManager.getPackageInfo(
                application.packageName,
                PackageManager.GET_PERMISSIONS,
            )
        val requestedPermissions =
            requireNotNull(packageInfo.requestedPermissions) {
                "the merged manifest must declare requested permissions"
            }

        // androidx.core merges an app-private signature permission into the manifest; only the
        // platform permissions are the v1 request set this test traces.
        return requestedPermissions
            .filter { permission -> permission.startsWith(PLATFORM_PERMISSION_PREFIX) }
            .toSet()
    }

    private companion object {
        const val PLATFORM_PERMISSION_PREFIX = "android.permission."
        const val LEGACY_READ_MAX_SDK_INT = 32
        const val POST_CAP_SDK_INT = MediaPermissionPolicy.PER_TYPE_MEDIA_PERMISSIONS_SDK_INT
        const val INTERNET_PERMISSION = "android.permission.INTERNET"
        const val WAKE_LOCK_PERMISSION = "android.permission.WAKE_LOCK"
        const val ACCESS_NETWORK_STATE_PERMISSION = "android.permission.ACCESS_NETWORK_STATE"
        const val RECEIVE_BOOT_COMPLETED_PERMISSION = "android.permission.RECEIVE_BOOT_COMPLETED"
        const val FOREGROUND_SERVICE_PERMISSION = "android.permission.FOREGROUND_SERVICE"
        const val FOREGROUND_SERVICE_DATA_SYNC_PERMISSION =
            "android.permission.FOREGROUND_SERVICE_DATA_SYNC"
        val CAPPED_PLATFORM_PERMISSIONS =
            setOf(
                MediaPermissionPolicy.READ_MEDIA_IMAGES,
                MediaPermissionPolicy.READ_MEDIA_VIDEO,
                MediaPermissionPolicy.READ_MEDIA_VISUAL_USER_SELECTED,
                MediaPermissionPolicy.READ_EXTERNAL_STORAGE,
                INTERNET_PERMISSION,
                NotificationPermissionPolicy.POST_NOTIFICATIONS,
                FOREGROUND_SERVICE_DATA_SYNC_PERMISSION,
                WAKE_LOCK_PERMISSION,
                ACCESS_NETWORK_STATE_PERMISSION,
                RECEIVE_BOOT_COMPLETED_PERMISSION,
                FOREGROUND_SERVICE_PERMISSION,
            )
        val POST_CAP_PLATFORM_PERMISSIONS =
            setOf(
                MediaPermissionPolicy.READ_MEDIA_IMAGES,
                MediaPermissionPolicy.READ_MEDIA_VIDEO,
                MediaPermissionPolicy.READ_MEDIA_VISUAL_USER_SELECTED,
                INTERNET_PERMISSION,
                NotificationPermissionPolicy.POST_NOTIFICATIONS,
                FOREGROUND_SERVICE_DATA_SYNC_PERMISSION,
                WAKE_LOCK_PERMISSION,
                ACCESS_NETWORK_STATE_PERMISSION,
                RECEIVE_BOOT_COMPLETED_PERMISSION,
                FOREGROUND_SERVICE_PERMISSION,
            )
    }
}
