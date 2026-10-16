package dev.homecloud.android.permissions

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import dev.homecloud.android.R

private const val PACKAGE_URI_SCHEME = "package"

/**
 * Renders the media-read permission surface for the activity. It reflects whether the client may
 * read photos and videos: while access can still be asked for it requests exactly the missing
 * media-read permissions and reflects the answer, and once access was permanently refused it sends
 * the user to the app's system settings instead of asking again.
 */
@Composable
internal fun MediaPermissionRequestContent(
    activity: ComponentActivity,
    modifier: Modifier = Modifier,
) {
    var hasRequested by rememberSaveable { mutableStateOf(false) }
    var revision by remember { mutableStateOf(0) }

    val status = remember(revision, hasRequested) {
        MediaPermissionRequestPolicy.status(
            Build.VERSION.SDK_INT,
            grantedPermissions(activity),
            permanentlyDeniedPermissions(activity, hasRequested),
        )
    }

    val permissionsLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions(),
    ) { _ ->
        hasRequested = true
        revision += 1
    }

    DisposableEffect(activity) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) revision += 1
        }
        activity.lifecycle.addObserver(observer)
        onDispose { activity.lifecycle.removeObserver(observer) }
    }

    MediaPermissionPrompt(
        status = status,
        onRequestPermissions = {
            permissionsLauncher.launch(
                MediaPermissionRequestPolicy
                    .pendingPermissions(Build.VERSION.SDK_INT, grantedPermissions(activity))
                    .toTypedArray(),
            )
        },
        onOpenSettings = { openAppSettings(activity) },
        modifier = modifier,
    )
}

/**
 * Shows the message for the given status and, when access was not granted, the single action that
 * either asks for the missing permissions or opens the app's system settings.
 */
@Composable
private fun MediaPermissionPrompt(
    status: MediaPermissionStatus,
    onRequestPermissions: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val message = when (status) {
        MediaPermissionStatus.GRANTED -> stringResource(R.string.media_permission_granted_message)
        MediaPermissionStatus.REQUESTABLE -> stringResource(R.string.media_permission_request_message)
        MediaPermissionStatus.BLOCKED -> stringResource(R.string.media_permission_blocked_message)
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
    ) {
        Text(text = message, style = MaterialTheme.typography.bodyLarge)
        when (status) {
            MediaPermissionStatus.REQUESTABLE -> Button(onClick = onRequestPermissions) {
                Text(text = stringResource(R.string.media_permission_request_action))
            }

            MediaPermissionStatus.BLOCKED -> Button(onClick = onOpenSettings) {
                Text(text = stringResource(R.string.media_permission_settings_action))
            }

            MediaPermissionStatus.GRANTED -> Unit
        }
    }
}

/** Collects the required media-read permissions that are currently granted to the context. */
private fun grantedPermissions(context: Context): Set<String> =
    MediaPermissionPolicy.requiredPermissions(Build.VERSION.SDK_INT)
        .filter { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }
        .toSet()

/**
 * Collects the required media-read permissions the user permanently refused. Before the first ask
 * every permission without a rationale was never requested rather than refused, so the set stays
 * empty until a request has been answered.
 */
private fun permanentlyDeniedPermissions(
    activity: ComponentActivity,
    hasRequested: Boolean,
): Set<String> =
    if (hasRequested) {
        MediaPermissionPolicy.requiredPermissions(Build.VERSION.SDK_INT)
            .filterNot { activity.shouldShowRequestPermissionRationale(it) }
            .toSet()
    } else {
        emptySet()
    }

/** Opens the app's system details settings page so the user can change the refused permission. */
private fun openAppSettings(context: Context) {
    val intent = Intent(
        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
        Uri.fromParts(PACKAGE_URI_SCHEME, context.packageName, null),
    )
    context.startActivity(intent)
}
