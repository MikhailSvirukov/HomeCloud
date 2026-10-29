package dev.homecloud.android

import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.work.WorkManager
import dev.homecloud.android.backup.BackupCycleComposition
import dev.homecloud.android.backup.BackupWorkScheduler
import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.MediaLibrarySummaryContent
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.notifications.NotificationPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionRequestContent
import dev.homecloud.android.queue.FileUploadQueueStore
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.setup.FileSetupConfigStore
import dev.homecloud.android.setup.SetupConfigStore
import dev.homecloud.android.setup.SetupContent
import dev.homecloud.android.setup.SetupPrerequisites
import dev.homecloud.android.setup.SetupStateMachine
import dev.homecloud.android.setup.SetupSubmission
import dev.homecloud.android.status.BackupStatus
import dev.homecloud.android.status.BackupStatusContent
import dev.homecloud.android.status.BackupStatusReader
import dev.homecloud.android.status.CycleOutcome
import dev.homecloud.android.status.CycleState
import dev.homecloud.android.status.FileLastCycleStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException
import kotlin.coroutines.cancellation.CancellationException

/** Name of the app-private file that keeps the setup values across restarts. */
private const val SETUP_CONFIG_FILE_NAME = "setup.conf"

/**
 * Main screen that ensures the repeating backup cycle is scheduled when the app starts
 * (`CON-AND-001 / 1`) and asks on a fresh launch for the notification permission when the platform
 * requires it (`FR-AND-035`) without blocking the media surface when it is refused (`FR-AND-036`).
 * It hosts the setup surface next to the media permission surface and library summary
 * (`FR-AND-040 / 2`, `FR-AND-041 / 2`, `FR-AND-042 / 1`, `FR-AND-043 / 1`), restoring the saved
 * address, port, login and mode on start and persisting them again after a valid submission. The
 * backup status surface (`FR-AND-044 / 1`, `FR-AND-045 / 1`, `FR-AND-046 / 1`, `FR-AND-047 / 1`)
 * reads the queue snapshot and the recorded last cycle off the main thread once, when it enters
 * composition; a cycle that runs later is reflected on the next visit to the screen. The
 * registration and authentication layer does not exist yet, so the authentication prerequisite
 * stays unmet until it does.
 */
class MainActivity : ComponentActivity() {
    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) {
            requestNotificationPermission()
        }
        BackupWorkScheduler(WorkManager.getInstance(this)).schedulePeriodic()
        enableEdgeToEdge()
        val mediaDiscovery = MediaDiscovery(MediaStoreMediaRowSource(contentResolver))
        val setupStore = FileSetupConfigStore(File(filesDir, SETUP_CONFIG_FILE_NAME))
        setContent {
            MaterialTheme {
                Surface {
                    MediaPermissionRequestContent(activity = this@MainActivity) {
                        MediaLibrarySummaryContent(discovery = mediaDiscovery)
                        BackupStatusSection(filesDir = filesDir)
                        SetupSection(
                            store = setupStore,
                            hasMediaAccess = hasMediaAccess(),
                        )
                    }
                }
            }
        }
    }

    private fun requestNotificationPermission() {
        val granted =
            ContextCompat.checkSelfPermission(this, NotificationPermissionPolicy.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED
        if (NotificationPermissionPolicy.shouldRequest(Build.VERSION.SDK_INT, granted)) {
            notificationPermissionLauncher.launch(NotificationPermissionPolicy.POST_NOTIFICATIONS)
        }
    }

    private fun hasMediaAccess(): Boolean =
        MediaPermissionPolicy
            .requiredPermissions(Build.VERSION.SDK_INT)
            .any { permission ->
                ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED
            }
}

/**
 * Loads the backup status once when it enters composition and renders it next to the setup
 * content, so the screen shows the cycle state, the last completed cycle, the pending version count
 * and the last error category (`FR-AND-044 / 1`, `FR-AND-045 / 1`, `FR-AND-046 / 1`,
 * `FR-AND-047 / 1`). Reading stays off the main thread; nothing is shown until the read finishes.
 */
@Composable
private fun BackupStatusSection(filesDir: File) {
    var status by remember(filesDir) { mutableStateOf<BackupStatus?>(null) }
    LaunchedEffect(filesDir) {
        status = readBackupStatus(filesDir)
    }
    val current = status
    if (current != null) {
        BackupStatusContent(current)
    }
}

/** Reads the status off the main thread, keeping the surface away on an unexpected read failure. */
private suspend fun readBackupStatus(filesDir: File): BackupStatus? =
    try {
        withContext(Dispatchers.IO) { statusFrom(filesDir) }
    } catch (cancellation: CancellationException) {
        throw cancellation
    } catch (_: Exception) {
        null
    }

/**
 * Builds the status from the persisted queue and last cycle. An unreadable queue snapshot counts
 * as no queued versions, and a missing record leaves the cycle idle.
 */
private fun statusFrom(filesDir: File): BackupStatus {
    val lastCycle = FileLastCycleStore(File(filesDir, BackupCycleComposition.LAST_CYCLE_FILE_NAME)).read()
    val entries = readQueueSnapshot(filesDir)
    val state =
        when (lastCycle?.outcome) {
            CycleOutcome.SUCCESS -> CycleState.COMPLETED
            CycleOutcome.FAILURE -> CycleState.FAILED
            null -> CycleState.IDLE
        }
    return BackupStatusReader.statusOf(entries, lastCycle, state)
}

/** Loads the persisted queue, treating an unreadable snapshot as an empty queue. */
private fun readQueueSnapshot(filesDir: File): List<UploadQueueEntry> =
    try {
        FileUploadQueueStore(File(filesDir, BackupCycleComposition.QUEUE_SNAPSHOT_FILE_NAME)).read()
    } catch (_: Exception) {
        emptyList()
    }

/**
 * Holds the setup draft for the current screen, restores the saved address, port, login and mode
 * when it first enters composition (`FR-AND-040 / 2`) and persists a valid submission without
 * blocking the main thread (`FR-AND-042 / 1`, `CFG-CL-029 / 1`). The credentials of a submission
 * stay in the submission object only: the configuration is saved and the password is never stored
 * or logged (`FR-AND-041 / 2`).
 */
@Composable
private fun SetupSection(
    store: SetupConfigStore,
    hasMediaAccess: Boolean,
) {
    var state by remember(store) {
        mutableStateOf(
            SetupStateMachine.initialState(
                saved = store.load(),
                prerequisites = SetupPrerequisites(hasMediaAccess = hasMediaAccess, isAuthenticated = false),
            ),
        )
    }
    val scope = rememberCoroutineScope()
    val current =
        state.copy(
            prerequisites = SetupPrerequisites(hasMediaAccess = hasMediaAccess, isAuthenticated = false),
        )
    SetupContent(
        state = current,
        onDraftChange = { draft -> state = SetupStateMachine.edit(state, draft) },
        onSubmit = {
            val submission = SetupStateMachine.submit(current)
            if (submission != null) {
                scope.launch {
                    if (persist(store, submission)) {
                        state = SetupStateMachine.saved(state, submission.configuration)
                    }
                }
            }
        },
    )
}

/** Writes the submitted configuration off the main thread and reports whether it was stored. */
private suspend fun persist(
    store: SetupConfigStore,
    submission: SetupSubmission,
): Boolean =
    try {
        withContext(Dispatchers.IO) { store.save(submission.configuration) }
        true
    } catch (cancellation: CancellationException) {
        throw cancellation
    } catch (failure: IOException) {
        false
    }
