package dev.homecloud.android.permissions

import dev.homecloud.android.R
import org.junit.Assert.assertEquals
import org.junit.Test

class MediaPermissionPromptTest {
    @Test
    fun `every required permission granted prompts with the granted message on both sides of api 33`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_granted_message, MediaPermissionAction.NONE),
            mediaPermissionPrompt(
                LEGACY_SDK_INT,
                setOf(MediaPermissionPolicy.READ_EXTERNAL_STORAGE),
                emptySet(),
                hasRequested = false,
            ),
        )
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_granted_message, MediaPermissionAction.NONE),
            mediaPermissionPrompt(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
                emptySet(),
                hasRequested = false,
            ),
        )
    }

    @Test
    fun `nothing granted before a request was answered prompts for access`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_request_message, MediaPermissionAction.REQUEST),
            mediaPermissionPrompt(
                LEGACY_SDK_INT,
                emptySet(),
                emptySet(),
                hasRequested = false,
            ),
        )
    }

    @Test
    fun `one missing per type permission before a request was answered prompts for access`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_request_message, MediaPermissionAction.REQUEST),
            mediaPermissionPrompt(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
                emptySet(),
                hasRequested = false,
            ),
        )
    }

    @Test
    fun `a permanently refused missing permission after a request prompts for settings`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_blocked_message, MediaPermissionAction.OPEN_SETTINGS),
            mediaPermissionPrompt(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
                hasRequested = true,
            ),
        )
    }

    @Test
    fun `a refused set before any request was answered still prompts for access`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_request_message, MediaPermissionAction.REQUEST),
            mediaPermissionPrompt(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
                hasRequested = false,
            ),
        )
    }

    @Test
    fun `a permanently refused permission that is already granted prompts with the granted message`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_granted_message, MediaPermissionAction.NONE),
            mediaPermissionPrompt(
                MODERN_SDK_INT,
                setOf(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
                hasRequested = true,
            ),
        )
    }

    @Test
    fun `visual user selected access at api 34 prompts with the limited coverage message and the request action`() {
        assertEquals(
            MediaPermissionPrompt(R.string.media_permission_limited_message, MediaPermissionAction.REQUEST),
            mediaPermissionPrompt(
                PARTIAL_ACCESS_SDK_INT,
                setOf(
                    MediaPermissionPolicy.READ_MEDIA_IMAGES,
                    MediaPermissionPolicy.READ_MEDIA_VISUAL_USER_SELECTED,
                ),
                setOf(MediaPermissionPolicy.READ_MEDIA_VIDEO),
                hasRequested = true,
            ),
        )
    }

    private companion object {
        const val LEGACY_SDK_INT = 32
        const val MODERN_SDK_INT = 33
        const val PARTIAL_ACCESS_SDK_INT = 34
    }
}
