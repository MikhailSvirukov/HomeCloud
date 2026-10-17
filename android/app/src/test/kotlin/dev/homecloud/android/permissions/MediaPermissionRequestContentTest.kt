package dev.homecloud.android.permissions

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import dev.homecloud.android.R
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/** First API level whose media reads are the per-type image and video permissions. */
private const val PER_TYPE_PERMISSIONS_SDK_INT = 33

/**
 * Renders [MediaPermissionRequestContent] on the JVM and asserts the message and the single action
 * the user actually sees for the platform permission state. The granted state is produced through
 * Robolectric's shadow application permissions, so no permission dialog is interacted with.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [PER_TYPE_PERMISSIONS_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class MediaPermissionRequestContentTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun `every required permission granted shows the granted message and offers no action`() {
        grant(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO)

        render()

        composeRule.onNodeWithText(grantedMessage()).assertIsDisplayed()
        composeRule.onAllNodes(hasClickAction()).assertCountEquals(0)
    }

    @Test
    fun `nothing granted shows the request message together with the request action`() {
        deny(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO)

        render()

        composeRule.onNodeWithText(requestMessage()).assertIsDisplayed()
        composeRule.onNodeWithText(requestAction()).assertIsDisplayed()
    }

    @Test
    fun `requesting access offers the request action as a single button`() {
        deny(MediaPermissionPolicy.READ_MEDIA_IMAGES, MediaPermissionPolicy.READ_MEDIA_VIDEO)

        render()

        composeRule.onAllNodes(hasClickAction()).assertCountEquals(1)
    }

    private fun render() {
        composeRule.setContent {
            MediaPermissionRequestContent(activity = composeRule.activity)
        }
    }

    private fun grant(vararg permissions: String) {
        shadowOf(RuntimeEnvironment.getApplication()).grantPermissions(*permissions)
    }

    private fun deny(vararg permissions: String) {
        shadowOf(RuntimeEnvironment.getApplication()).denyPermissions(*permissions)
    }

    private fun grantedMessage(): String = composeRule.activity.getString(R.string.media_permission_granted_message)

    private fun requestMessage(): String = composeRule.activity.getString(R.string.media_permission_request_message)

    private fun requestAction(): String = composeRule.activity.getString(R.string.media_permission_request_action)
}
