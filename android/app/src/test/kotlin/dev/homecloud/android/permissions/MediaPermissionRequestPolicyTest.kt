package dev.homecloud.android.permissions

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaPermissionRequestPolicyTest {

    @Test
    fun `status is granted when every required permission is granted on both sides of api 33`() {
        assertEquals(
            MediaPermissionStatus.GRANTED,
            MediaPermissionRequestPolicy.status(
                LEGACY_SDK_INT,
                setOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
                emptySet(),
            ),
        )
        assertEquals(
            MediaPermissionStatus.GRANTED,
            MediaPermissionRequestPolicy.status(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
                emptySet(),
            ),
        )
    }

    @Test
    fun `status is requestable when a required permission is missing and nothing is permanently denied`() {
        assertEquals(
            MediaPermissionStatus.REQUESTABLE,
            MediaPermissionRequestPolicy.status(LEGACY_SDK_INT, emptySet(), emptySet()),
        )
    }

    @Test
    fun `status is requestable when only one of the two per type permissions is missing`() {
        assertEquals(
            MediaPermissionStatus.REQUESTABLE,
            MediaPermissionRequestPolicy.status(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
                emptySet(),
            ),
        )
    }

    @Test
    fun `status is blocked when a missing required permission is permanently denied`() {
        assertEquals(
            MediaPermissionStatus.BLOCKED,
            MediaPermissionRequestPolicy.status(
                LEGACY_SDK_INT,
                emptySet(),
                setOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
            ),
        )
    }

    @Test
    fun `status is blocked when one required permission is permanently denied while another is missing`() {
        assertEquals(
            MediaPermissionStatus.BLOCKED,
            MediaPermissionRequestPolicy.status(
                MODERN_SDK_INT,
                emptySet(),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
            ),
        )
    }

    @Test
    fun `status is blocked when a granted permission is mixed with a permanently denied missing one`() {
        assertEquals(
            MediaPermissionStatus.BLOCKED,
            MediaPermissionRequestPolicy.status(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
            ),
        )
    }

    @Test
    fun `status stays granted when a permanently denied permission is already granted`() {
        assertEquals(
            MediaPermissionStatus.GRANTED,
            MediaPermissionRequestPolicy.status(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
            ),
        )
    }

    @Test
    fun `pending permissions lists missing required ones in policy order on both sides of api 33`() {
        assertEquals(
            listOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
            MediaPermissionRequestPolicy.pendingPermissions(LEGACY_SDK_INT, emptySet()),
        )
        assertEquals(
            listOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
            MediaPermissionRequestPolicy.pendingPermissions(MODERN_SDK_INT, emptySet()),
        )
        assertEquals(
            listOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
            MediaPermissionRequestPolicy.pendingPermissions(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
            ),
        )
        assertEquals(
            listOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
            MediaPermissionRequestPolicy.pendingPermissions(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
            ),
        )
    }

    @Test
    fun `pending permissions is empty when every required permission is granted`() {
        assertEquals(
            emptyList<String>(),
            MediaPermissionRequestPolicy.pendingPermissions(
                LEGACY_SDK_INT,
                setOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
            ),
        )
        assertEquals(
            emptyList<String>(),
            MediaPermissionRequestPolicy.pendingPermissions(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
            ),
        )
    }

    @Test
    fun `unrelated granted permission neither satisfies a requirement nor appears in pending permissions`() {
        assertEquals(
            listOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
            MediaPermissionRequestPolicy.pendingPermissions(
                MODERN_SDK_INT,
                setOf(UNRELATED_GRANTED_PERMISSION),
            ),
        )
    }

    private companion object {
        const val LEGACY_SDK_INT = 32
        const val MODERN_SDK_INT = 33
        const val UNRELATED_GRANTED_PERMISSION = "android.permission.READ_MEDIA_AUDIO"
    }
}
