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
 * read photos and videos: while access can still be asked for it opens one system request for the
 * permissions the policy reports, reflects the answer, and reports when only a user-selected subset
 * is available; once access was permanently refused it sends the user to the app's system settings
 * instead of asking again. Once full access is granted it also renders the supplied content below
 * the message.
 */
@Composable
internal fun MediaPermissionRequestContent(
    activity: ComponentActivity,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit = {},
) {
    var hasRequested by rememberSaveable { mutableStateOf(false) }
    var revision by remember { mutableStateOf(0) }

    val prompt =
        remember(revision, hasRequested) {
            mediaPermissionPrompt(
                sdkInt = Build.VERSION.SDK_INT,
                granted = grantedPermissions(activity),
                permanentlyDenied = permanentlyDeniedPermissions(activity),
                hasRequested = hasRequested,
            )
        }

    val permissionsLauncher =
        rememberLauncherForActivityResult(
            contract = ActivityResultContracts.RequestMultiplePermissions(),
        ) { _ ->
            hasRequested = true
            revision += 1
        }

    DisposableEffect(activity) {
        val observer =
            LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) revision += 1
            }
        activity.lifecycle.addObserver(observer)
        onDispose { activity.lifecycle.removeObserver(observer) }
    }

    MediaPermissionSurface(
        prompt = prompt,
        onRequestPermissions = {
            permissionsLauncher.launch(
                MediaPermissionPolicy
                    .requestedPermissions(Build.VERSION.SDK_INT)
                    .toTypedArray(),
            )
        },
        onOpenSettings = { openAppSettings(activity) },
        modifier = modifier,
        content = content,
    )
}

/**
 * Shows the prompt's message and, when access was not fully granted, the single action it carries.
 * Full access renders the supplied content in place of an action; selected access keeps offering
 * the request action so the user can change the selection.
 */
@Composable
private fun MediaPermissionSurface(
    prompt: MediaPermissionPrompt,
    onRequestPermissions: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit = {},
) {
    Column(
        modifier =
            modifier
                .fillMaxSize()
                .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
    ) {
        Text(text = stringResource(prompt.messageResId), style = MaterialTheme.typography.bodyLarge)
        when (prompt.action) {
            MediaPermissionAction.REQUEST ->
                Button(onClick = onRequestPermissions) {
                    Text(text = stringResource(R.string.media_permission_request_action))
                }

            MediaPermissionAction.OPEN_SETTINGS ->
                Button(onClick = onOpenSettings) {
                    Text(text = stringResource(R.string.media_permission_settings_action))
                }

            MediaPermissionAction.NONE -> content()
        }
    }
}

/** Collects the media-read permissions the policy requests that are currently granted to the context. */
private fun grantedPermissions(context: Context): Set<String> =
    MediaPermissionPolicy
        .requestedPermissions(Build.VERSION.SDK_INT)
        .filter { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }
        .toSet()

/**
 * Collects the required media-read permissions the activity reports as refused. Before the first
 * ask this also covers permissions that were never requested; the prompt decision only treats them
 * as refusals once a request has been answered.
 */
private fun permanentlyDeniedPermissions(activity: ComponentActivity): Set<String> =
    MediaPermissionPolicy
        .requiredPermissions(Build.VERSION.SDK_INT)
        .filterNot { activity.shouldShowRequestPermissionRationale(it) }
        .toSet()

/** Opens the app's system details settings page so the user can change the refused permission. */
private fun openAppSettings(context: Context) {
    val intent =
        Intent(
            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.fromParts(PACKAGE_URI_SCHEME, context.packageName, null),
        )
    context.startActivity(intent)
}
