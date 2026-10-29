package dev.homecloud.android.status

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import dev.homecloud.android.R
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.text.DateFormat
import java.util.Date

private const val BACKUP_STATUS_CONTENT_SDK_INT = 33

/**
 * Renders [BackupStatusContent] on the JVM and asserts what the user sees for `FR-AND-044 / 1`,
 * `FR-AND-045 / 1`, `FR-AND-046 / 1`, `FR-AND-047 / 1` and `FR-AND-048 / 1`: every cycle state,
 * the last completion line and result, the pending count, the error category, and the absence of
 * sensitive details or server-version actions.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [BACKUP_STATUS_CONTENT_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class BackupStatusContentTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun `idle state renders its label`() {
        assertStateLabel(CycleState.IDLE, R.string.backup_status_state_idle)
    }

    @Test
    fun `scanning state renders its label`() {
        assertStateLabel(CycleState.SCANNING, R.string.backup_status_state_scanning)
    }

    @Test
    fun `uploading state renders its label`() {
        assertStateLabel(CycleState.UPLOADING, R.string.backup_status_state_uploading)
    }

    @Test
    fun `completed state renders its label`() {
        assertStateLabel(CycleState.COMPLETED, R.string.backup_status_state_completed)
    }

    @Test
    fun `failed state renders its label`() {
        assertStateLabel(CycleState.FAILED, R.string.backup_status_state_failed)
    }

    @Test
    fun `status without a completion time shows that no cycle has completed yet`() {
        render(status(state = CycleState.IDLE))

        composeRule.onNodeWithText(string(R.string.backup_status_last_never)).assertIsDisplayed()
    }

    @Test
    fun `successful cycle shows the last completion time and success result`() {
        render(
            status(
                state = CycleState.COMPLETED,
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.SUCCESS,
            ),
        )

        composeRule.onNodeWithText(lastCompletedMessage(COMPLETED_AT_MILLIS)).assertIsDisplayed()
        composeRule.onNodeWithText(string(R.string.backup_status_result_success)).assertIsDisplayed()
    }

    @Test
    fun `failed cycle shows the last completion time and failure result`() {
        render(
            status(
                state = CycleState.FAILED,
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.FAILURE,
                error = LastErrorCategory.INTERNAL,
            ),
        )

        composeRule.onNodeWithText(lastCompletedMessage(COMPLETED_AT_MILLIS)).assertIsDisplayed()
        composeRule.onNodeWithText(string(R.string.backup_status_result_failure)).assertIsDisplayed()
    }

    @Test
    fun `one pending version uses the singular pending message`() {
        render(status(state = CycleState.IDLE, pendingVersions = SINGLE_PENDING_VERSION))

        composeRule.onNodeWithText(pendingMessage(SINGLE_PENDING_VERSION)).assertIsDisplayed()
    }

    @Test
    fun `two pending versions use the plural pending message`() {
        render(status(state = CycleState.IDLE, pendingVersions = SEVERAL_PENDING_VERSIONS))

        composeRule.onNodeWithText(pendingMessage(SEVERAL_PENDING_VERSIONS)).assertIsDisplayed()
    }

    @Test
    fun `media library failure renders its category message`() {
        assertCategoryMessage(
            LastErrorCategory.MEDIA_LIBRARY_UNREADABLE,
            R.string.backup_status_error_media_library_unreadable,
        )
    }

    @Test
    fun `queue failure renders its category message`() {
        assertCategoryMessage(
            LastErrorCategory.QUEUE_UNAVAILABLE,
            R.string.backup_status_error_queue_unavailable,
        )
    }

    @Test
    fun `transport failure renders its category message`() {
        assertCategoryMessage(
            LastErrorCategory.TRANSPORT_UNAVAILABLE,
            R.string.backup_status_error_transport_unavailable,
        )
    }

    @Test
    fun `internal failure renders its category message`() {
        assertCategoryMessage(
            LastErrorCategory.INTERNAL,
            R.string.backup_status_error_internal,
        )
    }

    @Test
    fun `failed cycle shows the category but no exception message, password, key, or file name`() {
        render(
            status(
                state = CycleState.FAILED,
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.FAILURE,
                error = LastErrorCategory.MEDIA_LIBRARY_UNREADABLE,
            ),
        )

        composeRule.onNodeWithText(string(R.string.backup_status_error_media_library_unreadable)).assertIsDisplayed()
        listOf(EXCEPTION_MESSAGE, PASSWORD, KEY_MATERIAL, FILE_NAME).forEach { secret ->
            composeRule.onAllNodesWithText(secret, substring = true).assertCountEquals(0)
        }
    }

    @Test
    fun `the status surface offers no view, download, restore, or delete action`() {
        render(
            status(
                state = CycleState.COMPLETED,
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.SUCCESS,
                pendingVersions = SEVERAL_PENDING_VERSIONS,
            ),
        )

        listOf(
            VIEW_ACTION,
            DOWNLOAD_ACTION,
            RESTORE_ACTION,
            DELETE_ACTION,
            string(R.string.setup_title),
            string(R.string.setup_submit_action),
            string(R.string.media_permission_request_action),
            string(R.string.media_permission_settings_action),
        ).forEach { forbidden ->
            composeRule
                .onAllNodesWithText(forbidden, substring = true, ignoreCase = true)
                .assertCountEquals(0)
        }
    }

    private fun assertStateLabel(
        state: CycleState,
        resource: Int,
    ) {
        render(status(state = state))

        composeRule.onNodeWithText(string(resource)).assertIsDisplayed()
    }

    private fun assertCategoryMessage(
        category: LastErrorCategory,
        resource: Int,
    ) {
        render(
            status(
                state = CycleState.FAILED,
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.FAILURE,
                error = category,
            ),
        )

        composeRule.onNodeWithText(string(resource)).assertIsDisplayed()
    }

    private fun render(status: BackupStatus) {
        composeRule.setContent {
            BackupStatusContent(status)
        }
    }

    private fun status(
        state: CycleState,
        completedAtMillis: Long? = null,
        outcome: CycleOutcome? = null,
        pendingVersions: Int = 0,
        error: LastErrorCategory? = null,
    ): BackupStatus =
        BackupStatus(
            state = state,
            lastCompletedAtMillis = completedAtMillis,
            lastOutcome = outcome,
            pendingVersions = pendingVersions,
            lastError = error,
        )

    private fun string(resource: Int): String = composeRule.activity.getString(resource)

    private fun lastCompletedMessage(millis: Long): String =
        composeRule.activity.getString(
            R.string.backup_status_last_completed,
            DateFormat.getDateTimeInstance().format(Date(millis)),
        )

    private fun pendingMessage(count: Int): String =
        composeRule.activity.resources.getQuantityString(R.plurals.backup_status_pending, count, count)

    private companion object {
        const val COMPLETED_AT_MILLIS = 1_700_000_000_000L
        const val SINGLE_PENDING_VERSION = 1
        const val SEVERAL_PENDING_VERSIONS = 2

        const val EXCEPTION_MESSAGE = "MediaStore could not be queried"
        const val PASSWORD = "correct-horse-battery-staple"
        const val KEY_MATERIAL = "BEGIN PRIVATE KEY"
        const val FILE_NAME = "holiday-photo.jpg"

        const val VIEW_ACTION = "View"
        const val DOWNLOAD_ACTION = "Download"
        const val RESTORE_ACTION = "Restore"
        const val DELETE_ACTION = "Delete"
    }
}
