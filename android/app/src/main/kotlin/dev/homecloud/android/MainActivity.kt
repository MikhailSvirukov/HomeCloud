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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.work.WorkManager
import dev.homecloud.android.backup.BackupWorkScheduler
import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.MediaLibrarySummaryContent
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.notifications.NotificationPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionRequestContent
import dev.homecloud.android.setup.FileSetupConfigStore
import dev.homecloud.android.setup.SetupConfigStore
import dev.homecloud.android.setup.SetupContent
import dev.homecloud.android.setup.SetupPrerequisites
import dev.homecloud.android.setup.SetupStateMachine
import dev.homecloud.android.setup.SetupSubmission
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
