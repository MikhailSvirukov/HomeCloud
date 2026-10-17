package dev.homecloud.android

import android.content.Intent
import android.provider.MediaStore
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.lifecycle.Lifecycle
import dev.homecloud.android.permissions.MediaPermissionPolicy
import dev.homecloud.android.testsupport.MediaTestData
import dev.homecloud.android.testsupport.RecordingMediaContentProvider
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Launches the activity the manifest declares and asserts the permission surface a first launch
 * shows, the media library count once access is granted, together with the launcher entry point the
 * package manager resolves.
 *
 * The Compose rule launches the activity before the test body runs, so the granted state is produced
 * through Robolectric's shadow application permissions and the lifecycle is driven back to resume,
 * where the surface recomputes, instead of recreating the activity.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [MainActivityTest.PER_TYPE_PERMISSIONS_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class MainActivityTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    @Test
    fun `launch without media access shows the request message and offers the request action`() {
        denyRequiredPermissions()

        composeRule.onNodeWithText(requestMessage()).assertIsDisplayed()
        composeRule.onNodeWithText(requestAction()).assertIsDisplayed()
    }

    @Test
    fun `launch with every media permission granted shows the granted message and offers no action`() {
        grantRequiredPermissions()
        resumeAgain()

        composeRule.onNodeWithText(grantedMessage()).assertIsDisplayed()
        composeRule.onAllNodes(hasClickAction()).assertCountEquals(0)
    }

    @Test
    fun `launch with every media permission granted shows the media library count`() {
        installMediaProvider(
            listOf(
                mediaRow(id = IMAGE_ID, mimeType = MediaTestData.IMAGE_MIME_TYPE),
                mediaRow(id = VIDEO_ID, mimeType = MediaTestData.VIDEO_MIME_TYPE),
            ),
        )
        grantRequiredPermissions()
        resumeAgain()

        waitForLibrarySummary()

        composeRule.onNodeWithText(foundMessage(EXPECTED_MEDIA_COUNT)).assertIsDisplayed()
    }

    @Test
    fun `launch with an unreadable media library shows the failure message`() {
        installUnreadableMediaProvider()
        grantRequiredPermissions()
        resumeAgain()

        waitForLibraryFailure()

        composeRule.onNodeWithText(failureMessage()).assertIsDisplayed()
    }

    @Test
    fun `the manifest launcher entry point resolves to MainActivity`() {
        val application = RuntimeEnvironment.getApplication()
        val launcherIntent =
            Intent(Intent.ACTION_MAIN)
                .addCategory(Intent.CATEGORY_LAUNCHER)
                .setPackage(application.packageName)

        val launcherActivities =
            application.packageManager
                .queryIntentActivities(launcherIntent, 0)
                .map { resolveInfo -> resolveInfo.activityInfo.name }

        assertEquals(listOf(MainActivity::class.java.name), launcherActivities)
    }

    private fun resumeAgain() {
        composeRule.activityRule.scenario.moveToState(Lifecycle.State.STARTED)
        composeRule.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        composeRule.waitForIdle()
    }

    private fun installMediaProvider(
        rows: List<Array<Any?>>,
        queryReturnsNull: Boolean = false,
    ) {
        val authority =
            requireNotNull(MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL).authority) {
                "media collection uri must carry an authority"
            }
        val provider =
            Robolectric
                .buildContentProvider(RecordingMediaContentProvider::class.java)
                .create(authority)
                .get()
        provider.rows = rows
        provider.queryReturnsNull = queryReturnsNull
    }

    private fun installUnreadableMediaProvider() {
        installMediaProvider(emptyList(), queryReturnsNull = true)
    }

    private fun mediaRow(
        id: Long,
        mimeType: String,
    ): Array<Any?> =
        arrayOf(
            id,
            DISPLAY_NAME_PREFIX + id,
            MediaTestData.SIZE_BYTES,
            MediaTestData.DATE_MODIFIED_EPOCH_SECONDS,
            mimeType,
        )

    private fun waitForLibrarySummary() {
        waitForText(foundMessage(EXPECTED_MEDIA_COUNT))
    }

    private fun waitForLibraryFailure() {
        waitForText(failureMessage())
    }

    private fun waitForText(text: String) {
        composeRule.waitUntil(timeoutMillis = LIBRARY_TIMEOUT_MILLIS) {
            composeRule
                .onAllNodesWithText(text)
                .fetchSemanticsNodes()
                .isNotEmpty()
        }
    }

    private fun foundMessage(count: Int): String =
        composeRule.activity.resources.getQuantityString(R.plurals.media_library_found, count, count)

    private fun failureMessage(): String = composeRule.activity.getString(R.string.media_library_failed)

    private fun grantRequiredPermissions() {
        val permissions = MediaPermissionPolicy.requiredPermissions(PER_TYPE_PERMISSIONS_SDK_INT)
        shadowOf(RuntimeEnvironment.getApplication()).grantPermissions(*permissions.toTypedArray())
    }

    private fun denyRequiredPermissions() {
        val permissions = MediaPermissionPolicy.requiredPermissions(PER_TYPE_PERMISSIONS_SDK_INT)
        shadowOf(RuntimeEnvironment.getApplication()).denyPermissions(*permissions.toTypedArray())
    }

    private fun grantedMessage(): String = composeRule.activity.getString(R.string.media_permission_granted_message)

    private fun requestMessage(): String = composeRule.activity.getString(R.string.media_permission_request_message)

    private fun requestAction(): String = composeRule.activity.getString(R.string.media_permission_request_action)

    private companion object {
        const val PER_TYPE_PERMISSIONS_SDK_INT = 33
        const val IMAGE_ID = 7L
        const val VIDEO_ID = 8L
        const val DISPLAY_NAME_PREFIX = "item-"
        const val EXPECTED_MEDIA_COUNT = 2
        const val LIBRARY_TIMEOUT_MILLIS = 5_000L
    }
}
