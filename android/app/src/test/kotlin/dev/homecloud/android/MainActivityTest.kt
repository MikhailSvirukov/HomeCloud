package dev.homecloud.android

import android.content.Context
import android.content.Intent
import android.provider.MediaStore
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ApplicationProvider
import androidx.work.WorkManager
import androidx.work.testing.WorkManagerTestInitHelper
import dev.homecloud.android.backup.BackupCycleComposition
import dev.homecloud.android.backup.BackupWorkScheduler
import dev.homecloud.android.notifications.NotificationPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionPolicy
import dev.homecloud.android.queue.FileUploadQueueStore
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.setup.FileSetupConfigStore
import dev.homecloud.android.setup.SetupConfiguration
import dev.homecloud.android.setup.SetupMode
import dev.homecloud.android.status.CycleOutcome
import dev.homecloud.android.status.FileLastCycleStore
import dev.homecloud.android.status.LastCycleRecord
import dev.homecloud.android.testsupport.MediaTestData
import dev.homecloud.android.testsupport.RecordingMediaContentProvider
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.ExternalResource
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.junit.runners.model.Statement
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.io.File
import java.text.DateFormat
import java.util.Date

/**
 * Launches the activity the manifest declares and asserts the permission surface a first launch
 * shows, the media library count once access is granted, together with the launcher entry point the
 * package manager resolves.
 *
 * The Compose rule launches the activity before the test body runs, so the granted state is produced
 * through Robolectric's shadow application permissions: media access is granted or denied in the
 * test body and the lifecycle is driven back to resume, while [NotificationPermissionRule] grants
 * or denies the notification permission before the activity launches. The activity schedules its
 * periodic backup cycle on create, so a test WorkManager is installed by an outer rule before the
 * activity launches and no real work is executed.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [MainActivityTest.PER_TYPE_PERMISSIONS_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class MainActivityTest {
    @get:Rule(order = 0)
    val notificationPermissionRule = NotificationPermissionRule()

    @get:Rule(order = 1)
    val testWorkManagerRule = TestWorkManagerRule()

    @get:Rule(order = 2)
    val backupStatusFilesRule = BackupStatusFilesRule(completedAtMillis = LAST_CYCLE_COMPLETED_AT_MILLIS)

    @get:Rule(order = 3)
    val setupConfigRule =
        SetupConfigFileRule(
            host = RESTORED_HOST,
            port = RESTORED_PORT,
            login = RESTORED_LOGIN,
        )

    @get:Rule(order = 4)
    val composeRule = createAndroidComposeRule<MainActivity>()

    @Test
    fun `launch without media access shows the request message and offers the request action`() {
        denyRequiredPermissions()

        composeRule.onNodeWithText(requestMessage()).assertIsDisplayed()
        composeRule.onNodeWithText(requestAction()).assertIsDisplayed()
    }

    @Test
    fun `launch on api 33 with a denied notification permission requests it once and keeps the media request surface`() {
        val activity = composeRule.activity
        val requestedPermission =
            requireNotNull(shadowOf(activity).lastRequestedPermission) {
                "a fresh launch must request the denied notification permission"
            }

        assertArrayEquals(
            arrayOf(NotificationPermissionPolicy.POST_NOTIFICATIONS),
            requestedPermission.requestedPermissions,
        )
        resumeAgain()
        assertSame(
            "the permission must be requested on create, not again on resume",
            requestedPermission,
            shadowOf(activity).lastRequestedPermission,
        )
        composeRule.onNodeWithText(requestMessage()).assertIsDisplayed()
        composeRule.onNodeWithText(requestAction()).assertIsDisplayed()
    }

    @Test
    fun `launch on api 33 with a denied notification permission still enqueues the periodic backup cycle`() {
        val workInfos =
            WorkManager
                .getInstance(composeRule.activity)
                .getWorkInfosForUniqueWork(BackupWorkScheduler.PERIODIC_WORK_NAME)
                .get()

        assertTrue(
            "FR-AND-036 forbids a denied notification permission from blocking backup",
            workInfos.isNotEmpty(),
        )
    }

    @Test
    @NotificationPermissionGranted
    fun `launch on api 33 with the notification permission granted records no permission request`() {
        assertNull(
            "a granted notification permission must not be requested",
            shadowOf(composeRule.activity).lastRequestedPermission,
        )
    }

    @Test
    @Config(sdk = [LEGACY_SDK_INT])
    fun `launch below api 33 records no notification permission request`() {
        assertNull(
            "the notification permission does not exist before api 33",
            shadowOf(composeRule.activity).lastRequestedPermission,
        )
    }

    @Test
    fun `launch with every media permission granted shows the granted message and offers no permission action`() {
        grantRequiredPermissions()
        resumeAgain()

        composeRule.onNodeWithText(grantedMessage()).assertIsDisplayed()
        composeRule.onAllNodesWithText(requestAction()).assertCountEquals(0)
        composeRule.onAllNodesWithText(settingsAction()).assertCountEquals(0)
    }

    @Test
    fun `launch displays the setup host saved before the activity started`() {
        grantRequiredPermissions()
        resumeAgain()

        waitForText(RESTORED_HOST)

        composeRule.onNodeWithText(RESTORED_HOST).assertIsDisplayed()
    }

    @Test
    @BackupStatusRecorded
    fun `launch shows the recorded last cycle result and pending count after the asynchronous read`() {
        grantRequiredPermissions()
        resumeAgain()

        val completedMessage = lastCompletedMessage(LAST_CYCLE_COMPLETED_AT_MILLIS)
        waitForText(completedMessage)

        composeRule.onNodeWithText(completedMessage).assertIsDisplayed()
        composeRule.onNodeWithText(composeRule.activity.getString(R.string.backup_status_state_completed)).assertIsDisplayed()
        composeRule.onNodeWithText(composeRule.activity.getString(R.string.backup_status_result_success)).assertIsDisplayed()
        composeRule.onNodeWithText(pendingMessage(EXPECTED_PENDING_VERSIONS)).assertIsDisplayed()
    }

    @Test
    fun `launch without a recorded cycle shows that no cycle has completed yet`() {
        grantRequiredPermissions()
        resumeAgain()

        val neverMessage = composeRule.activity.getString(R.string.backup_status_last_never)
        waitForText(neverMessage)

        composeRule.onNodeWithText(neverMessage).assertIsDisplayed()
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
            MediaTestData.DATE_TAKEN_EPOCH_MILLIS,
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

    private fun lastCompletedMessage(millis: Long): String =
        composeRule.activity.getString(
            R.string.backup_status_last_completed,
            DateFormat.getDateTimeInstance().format(Date(millis)),
        )

    private fun pendingMessage(count: Int): String =
        composeRule.activity.resources.getQuantityString(R.plurals.backup_status_pending, count, count)

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

    private fun settingsAction(): String = composeRule.activity.getString(R.string.media_permission_settings_action)

    private companion object {
        const val PER_TYPE_PERMISSIONS_SDK_INT = 33
        const val LEGACY_SDK_INT = 32
        const val IMAGE_ID = 7L
        const val VIDEO_ID = 8L
        const val DISPLAY_NAME_PREFIX = "item-"
        const val EXPECTED_MEDIA_COUNT = 2
        const val LIBRARY_TIMEOUT_MILLIS = 5_000L

        const val RESTORED_HOST = "198.51.100.7"
        const val RESTORED_PORT = 9000
        const val RESTORED_LOGIN = "saved-user"

        const val LAST_CYCLE_COMPLETED_AT_MILLIS = 1_700_000_000_000L
        const val EXPECTED_PENDING_VERSIONS = 2
    }
}

/** Marks a test that must launch the activity with the notification permission already granted. */
@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.FUNCTION)
private annotation class NotificationPermissionGranted

/** Marks a test that must launch the activity with a completed last cycle and a queue snapshot. */
@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.FUNCTION)
private annotation class BackupStatusRecorded

/**
 * Applies the notification permission state before the Compose rule launches the activity, so the
 * create-time request decision observes the platform state the test scenario describes. Tests
 * without [NotificationPermissionGranted] see the permission denied.
 */
class NotificationPermissionRule : TestRule {
    override fun apply(
        base: Statement,
        description: Description,
    ): Statement =
        object : Statement() {
            override fun evaluate() {
                val shadowApplication = shadowOf(RuntimeEnvironment.getApplication())
                if (description.getAnnotation(NotificationPermissionGranted::class.java) == null) {
                    shadowApplication.denyPermissions(NotificationPermissionPolicy.POST_NOTIFICATIONS)
                } else {
                    shadowApplication.grantPermissions(NotificationPermissionPolicy.POST_NOTIFICATIONS)
                }
                base.evaluate()
            }
        }
}

/**
 * Installs a test [androidx.work.WorkManager] before the activity is launched, so the periodic
 * cycle the activity schedules on create stays enqueued in the test scheduler instead of starting
 * real background work.
 */
class TestWorkManagerRule : ExternalResource() {
    override fun before() {
        WorkManagerTestInitHelper.initializeTestWorkManager(ApplicationProvider.getApplicationContext<Context>())
    }
}

/** Writes the setup configuration that the activity restores before the Compose rule launches it. */
class SetupConfigFileRule(
    private val host: String,
    private val port: Int,
    private val login: String,
) : ExternalResource() {
    override fun before() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        FileSetupConfigStore(File(context.filesDir, SETUP_CONFIG_FILE_NAME))
            .save(SetupConfiguration(host = host, port = port, login = login, mode = SetupMode.CONNECT))
    }

    override fun after() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        File(context.filesDir, SETUP_CONFIG_FILE_NAME).delete()
    }
}

/**
 * Writes the last-cycle record and upload queue snapshot before the Compose rule launches the
 * activity when the test is marked [BackupStatusRecorded], and removes both files afterwards so a
 * test without the marker launches with no recorded cycle.
 */
class BackupStatusFilesRule(
    private val completedAtMillis: Long,
) : TestRule {
    override fun apply(
        base: Statement,
        description: Description,
    ): Statement =
        object : Statement() {
            override fun evaluate() {
                val context = ApplicationProvider.getApplicationContext<Context>()
                val lastCycleFile = File(context.filesDir, BackupCycleComposition.LAST_CYCLE_FILE_NAME)
                val queueFile = File(context.filesDir, BackupCycleComposition.QUEUE_SNAPSHOT_FILE_NAME)
                if (description.getAnnotation(BackupStatusRecorded::class.java) != null) {
                    FileLastCycleStore(lastCycleFile)
                        .write(LastCycleRecord(completedAtMillis, CycleOutcome.SUCCESS, null))
                    FileUploadQueueStore(queueFile).write(snapshotEntries())
                } else {
                    lastCycleFile.delete()
                    queueFile.delete()
                }
                try {
                    base.evaluate()
                } finally {
                    lastCycleFile.delete()
                    queueFile.delete()
                }
            }
        }

    private fun snapshotEntries(): List<UploadQueueEntry> =
        listOf(
            snapshotEntry(PENDING_SOURCE_ID, UploadQueueState.PENDING),
            snapshotEntry(IN_FLIGHT_SOURCE_ID, UploadQueueState.IN_FLIGHT),
            snapshotEntry(COMPLETED_SOURCE_ID, UploadQueueState.COMPLETED),
            snapshotEntry(FAILED_SOURCE_ID, UploadQueueState.FAILED),
        )

    private fun snapshotEntry(
        sourceId: Long,
        state: UploadQueueState,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = SNAPSHOT_UPLOAD_ID_PREFIX + sourceId,
            sourceId = sourceId,
            identity = MediaTestData.versionIdentity(sourceId = sourceId),
            sizeBytes = MediaTestData.SIZE_BYTES,
            attemptCount = 0,
            state = state,
        )

    private companion object {
        const val SNAPSHOT_UPLOAD_ID_PREFIX = "snapshot-"
        const val PENDING_SOURCE_ID = 101L
        const val IN_FLIGHT_SOURCE_ID = 102L
        const val COMPLETED_SOURCE_ID = 103L
        const val FAILED_SOURCE_ID = 104L
    }
}

private const val SETUP_CONFIG_FILE_NAME = "setup.conf"
