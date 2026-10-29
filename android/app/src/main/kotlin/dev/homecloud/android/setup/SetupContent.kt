package dev.homecloud.android.setup

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import dev.homecloud.android.R

/**
 * Renders the setup surface (`FR-AND-040 / 2`, `FR-AND-041 / 2`, `FR-AND-042 / 1`,
 * `FR-AND-043 / 1`): server host and port, account login and password, the registration or
 * connection choice, inline field errors and the remaining backup prerequisites. The password is
 * masked, the submit action stays disabled until the draft is valid, and the only actions are the
 * ones the caller supplies, so setup never offers a view, download, restore or delete action
 * (`FR-AND-048 / 1`). The composable keeps no state of its own and performs no network or storage
 * work.
 */
@Composable
internal fun SetupContent(
    state: SetupState,
    onDraftChange: (SetupDraft) -> Unit,
    onSubmit: () -> Unit,
) {
    Column(
        modifier =
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(text = stringResource(R.string.setup_title), style = MaterialTheme.typography.headlineSmall)
        SetupFieldInput(
            label = stringResource(R.string.setup_host_label),
            hint = stringResource(R.string.setup_host_hint),
            value = state.draft.host,
            error = state.validation.errors[SetupField.HOST],
            keyboardType = KeyboardType.Uri,
            onValueChange = { value -> onDraftChange(state.draft.copy(host = value)) },
        )
        SetupFieldInput(
            label = stringResource(R.string.setup_port_label),
            hint = stringResource(R.string.setup_port_hint),
            value = state.draft.port,
            error = state.validation.errors[SetupField.PORT],
            keyboardType = KeyboardType.Number,
            onValueChange = { value -> onDraftChange(state.draft.copy(port = value)) },
        )
        SetupFieldInput(
            label = stringResource(R.string.setup_login_label),
            value = state.draft.login,
            error = state.validation.errors[SetupField.LOGIN],
            keyboardType = KeyboardType.Ascii,
            onValueChange = { value -> onDraftChange(state.draft.copy(login = value)) },
        )
        SetupFieldInput(
            label = stringResource(R.string.setup_password_label),
            value = state.draft.password,
            error = state.validation.errors[SetupField.PASSWORD],
            keyboardType = KeyboardType.Password,
            visualTransformation = PasswordVisualTransformation(),
            onValueChange = { value -> onDraftChange(state.draft.copy(password = value)) },
        )
        Text(text = stringResource(R.string.setup_mode_label), style = MaterialTheme.typography.titleSmall)
        Column(modifier = Modifier.selectableGroup()) {
            SetupMode.entries.forEach { mode ->
                SetupModeOption(
                    mode = mode,
                    selected = state.draft.mode == mode,
                    onSelect = { onDraftChange(state.draft.copy(mode = mode)) },
                )
            }
        }
        if (!state.prerequisites.isReadyForBackup) {
            Text(
                text = stringResource(R.string.setup_prerequisites_title),
                style = MaterialTheme.typography.titleSmall,
            )
            SetupPrerequisite(
                met = state.prerequisites.hasMediaAccess,
                message = R.string.setup_prerequisite_media_access_missing,
            )
            SetupPrerequisite(
                met = state.prerequisites.isAuthenticated,
                message = R.string.setup_prerequisite_authentication_missing,
            )
        }
        Button(
            onClick = onSubmit,
            enabled = state.validation.isValid,
        ) {
            Text(text = stringResource(R.string.setup_submit_action))
        }
    }
}

/** One labelled text field with its first error shown below it. */
@Composable
private fun SetupFieldInput(
    label: String,
    value: String,
    error: SetupError?,
    keyboardType: KeyboardType,
    onValueChange: (String) -> Unit,
    hint: String? = null,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(text = label) },
        placeholder =
            if (hint == null) {
                null
            } else {
                { Text(text = hint) }
            },
        isError = error != null,
        supportingText =
            if (error == null) {
                null
            } else {
                { Text(text = stringResource(errorMessageResource(error))) }
            },
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
        visualTransformation = visualTransformation,
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
}

/** One radio-button row of the single-choice setup mode selector. */
@Composable
private fun SetupModeOption(
    mode: SetupMode,
    selected: Boolean,
    onSelect: () -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .selectable(selected = selected, onClick = onSelect, role = Role.RadioButton),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Text(
            text = stringResource(mode.labelResource()),
            modifier = Modifier.padding(start = 8.dp),
        )
    }
}

/** One unmet backup prerequisite; a met prerequisite renders nothing. */
@Composable
private fun SetupPrerequisite(
    met: Boolean,
    message: Int,
) {
    if (!met) {
        Text(text = stringResource(message), style = MaterialTheme.typography.bodyMedium)
    }
}

private fun SetupMode.labelResource(): Int =
    when (this) {
        SetupMode.REGISTER -> R.string.setup_mode_register
        SetupMode.CONNECT -> R.string.setup_mode_connect
    }

private fun errorMessageResource(error: SetupError): Int =
    when (error) {
        SetupError.REQUIRED -> R.string.setup_error_required
        SetupError.NOT_AN_IP_LITERAL -> R.string.setup_error_not_an_ip_literal
        SetupError.PORT_OUT_OF_RANGE -> R.string.setup_error_port_out_of_range
        SetupError.LOGIN_CHARACTERS -> R.string.setup_error_login_characters
        SetupError.LOGIN_TOO_SHORT -> R.string.setup_error_login_too_short
        SetupError.LOGIN_TOO_LONG -> R.string.setup_error_login_too_long
        SetupError.PASSWORD_TOO_SHORT -> R.string.setup_error_password_too_short
        SetupError.PASSWORD_TOO_LONG -> R.string.setup_error_password_too_long
    }
