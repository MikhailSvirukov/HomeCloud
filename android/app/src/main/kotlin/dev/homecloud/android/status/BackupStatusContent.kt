package dev.homecloud.android.status

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.homecloud.android.R
import java.text.DateFormat
import java.util.Date

/**
 * Shows the backup cycle state, the time and result of the last completed cycle, the number of
 * versions waiting to upload, and the class of the last error (`FR-AND-044 / 1`, `FR-AND-045 / 1`,
 * `FR-AND-046 / 1`, `FR-AND-047 / 1`).
 *
 * Every line comes from a string resource, and the error line names only a category, so no
 * exception message, password, key, or file name can reach the screen. The surface offers no view,
 * download, restore, or delete action (`FR-AND-048 / 1`) and performs no storage or network work.
 */
@Composable
internal fun BackupStatusContent(status: BackupStatus) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text = stringResource(R.string.backup_status_title),
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            text = stringResource(status.state.labelResource()),
            style = MaterialTheme.typography.bodyLarge,
        )
        val completedAtMillis = status.lastCompletedAtMillis
        if (completedAtMillis == null) {
            Text(
                text = stringResource(R.string.backup_status_last_never),
                style = MaterialTheme.typography.bodyMedium,
            )
        } else {
            Text(
                text =
                    stringResource(
                        R.string.backup_status_last_completed,
                        formatCompletionTime(completedAtMillis),
                    ),
                style = MaterialTheme.typography.bodyMedium,
            )
            val outcome = status.lastOutcome
            if (outcome != null) {
                Text(
                    text = stringResource(outcome.resultResource()),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
        Text(
            text =
                pluralStringResource(
                    R.plurals.backup_status_pending,
                    status.pendingVersions,
                    status.pendingVersions,
                ),
            style = MaterialTheme.typography.bodyMedium,
        )
        val lastError = status.lastError
        if (lastError != null) {
            Text(
                text = stringResource(lastError.messageResource()),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

/** Formats a completion instant in the device locale and zone. */
@Composable
private fun formatCompletionTime(millis: Long): String =
    remember(millis) {
        DateFormat.getDateTimeInstance().format(Date(millis))
    }

private fun CycleState.labelResource(): Int =
    when (this) {
        CycleState.IDLE -> R.string.backup_status_state_idle
        CycleState.SCANNING -> R.string.backup_status_state_scanning
        CycleState.UPLOADING -> R.string.backup_status_state_uploading
        CycleState.COMPLETED -> R.string.backup_status_state_completed
        CycleState.FAILED -> R.string.backup_status_state_failed
    }

private fun CycleOutcome.resultResource(): Int =
    when (this) {
        CycleOutcome.SUCCESS -> R.string.backup_status_result_success
        CycleOutcome.FAILURE -> R.string.backup_status_result_failure
    }

private fun LastErrorCategory.messageResource(): Int =
    when (this) {
        LastErrorCategory.MEDIA_LIBRARY_UNREADABLE -> R.string.backup_status_error_media_library_unreadable
        LastErrorCategory.QUEUE_UNAVAILABLE -> R.string.backup_status_error_queue_unavailable
        LastErrorCategory.TRANSPORT_UNAVAILABLE -> R.string.backup_status_error_transport_unavailable
        LastErrorCategory.INTERNAL -> R.string.backup_status_error_internal
    }
