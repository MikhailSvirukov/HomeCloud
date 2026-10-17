package dev.homecloud.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import dev.homecloud.android.media.MediaDiscovery
import dev.homecloud.android.media.MediaLibrarySummaryContent
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.permissions.MediaPermissionRequestContent

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
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
