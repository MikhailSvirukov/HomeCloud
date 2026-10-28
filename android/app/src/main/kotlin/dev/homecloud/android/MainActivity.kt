package dev.homecloud.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.work.WorkManager
import dev.homecloud.android.backup.BackupWorkScheduler
import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.MediaLibrarySummaryContent
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.permissions.MediaPermissionRequestContent

/**
 * Main screen that ensures the repeating backup cycle is scheduled when the app starts
 * (`CON-AND-001 / 1`).
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
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
}
