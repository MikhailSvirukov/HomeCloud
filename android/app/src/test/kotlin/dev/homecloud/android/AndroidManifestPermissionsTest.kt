package dev.homecloud.android

import android.content.pm.PackageManager
import org.junit.Assert.assertEquals
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
 * It runs on the last API level that still takes the legacy storage read: the framework drops a
 * permission capped with `android:maxSdkVersion` from the request set on later levels, so this is
 * the level at which the whole declared set is observable.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [AndroidManifestPermissionsTest.LEGACY_READ_MAX_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class AndroidManifestPermissionsTest {
    @Test
    fun `manifest declares exactly the v1 platform permissions`() {
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
        val platformPermissions =
            requestedPermissions
                .filter { permission -> permission.startsWith(PLATFORM_PERMISSION_PREFIX) }
                .toSet()

        assertEquals(EXPECTED_PERMISSIONS, platformPermissions)
    }

    private companion object {
        const val PLATFORM_PERMISSION_PREFIX = "android.permission."
        const val LEGACY_READ_MAX_SDK_INT = 32
        val EXPECTED_PERMISSIONS =
            setOf(
                "android.permission.READ_MEDIA_IMAGES",
                "android.permission.READ_MEDIA_VIDEO",
                "android.permission.READ_EXTERNAL_STORAGE",
                "android.permission.READ_MEDIA_VISUAL_USER_SELECTED",
                "android.permission.INTERNET",
            )
    }
}
