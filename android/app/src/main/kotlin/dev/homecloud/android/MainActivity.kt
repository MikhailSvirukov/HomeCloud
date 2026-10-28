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
import androidx.core.content.ContextCompat
import androidx.work.WorkManager
import dev.homecloud.android.backup.BackupWorkScheduler
import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.MediaLibrarySummaryContent
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.notifications.NotificationPermissionPolicy
import dev.homecloud.android.permissions.MediaPermissionRequestContent

/**
 * Main screen that ensures the repeating backup cycle is scheduled when the app starts
 * (`CON-AND-001 / 1`) and asks once for the notification permission when the platform requires it
 * (`FR-AND-035`) without blocking the media surface when it is refused (`FR-AND-036`).
 */
class MainActivity : ComponentActivity() {
    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermission()
        BackupWorkScheduler(WorkManager.getInstance(this)).schedulePeriodic()
        enableEdgeToEdge()
        val mediaDiscovery = MediaDiscovery(MediaStoreMediaRowSource(contentResolver))
        setContent {
            MaterialTheme {
                Surface {
                    MediaPermissionRequestContent(activity = this) {
                        MediaLibrarySummaryContent(discovery = mediaDiscovery)
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
}
