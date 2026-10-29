package dev.homecloud.android.setup

/**
 * What must hold before backup may start (`FR-AND-043 / 1`): read access to at least one supported
 * media type and a completed registration or authentication.
 */
internal data class SetupPrerequisites(
    val hasMediaAccess: Boolean,
    val isAuthenticated: Boolean,
) {
    /** Whether both prerequisites are met. */
    val isReadyForBackup: Boolean
        get() = hasMediaAccess && isAuthenticated
}

/**
 * The setup surface state: the current draft, its validation, the backup prerequisites, the values
 * that were last saved and the configured credential bounds used to validate the draft
 * (`CFG-CL-029 / 1`). Every edit produces a new immutable state.
 */
internal data class SetupState(
    val draft: SetupDraft,
    val validation: SetupValidation,
    val prerequisites: SetupPrerequisites,
    val saved: SetupConfiguration?,
    val limits: SetupLimits = SetupValidator.defaultLimits,
) {
    /** Whether the user may start backup: the draft is valid and both prerequisites are met. */
    val canStartBackup: Boolean
        get() = validation.isValid && prerequisites.isReadyForBackup
}

/**
 * The setup transitions (`FR-AND-040 / 2`, `FR-AND-041 / 2`, `FR-AND-042 / 1`, `CFG-CL-029 / 1`):
 * opening a state from an optional saved configuration and the configured credential bounds,
 * editing the draft, and submitting it only once valid. Editing and submitting revalidate with the
 * bounds already held by the state, so a configuration change never mixes limits within one state.
 */
internal object SetupStateMachine {
    fun initialState(
        saved: SetupConfiguration?,
        prerequisites: SetupPrerequisites,
        limits: SetupLimits = SetupValidator.defaultLimits,
    ): SetupState {
        val draft =
            saved?.let { configuration ->
                SetupDraft(
                    host = configuration.host,
                    port = configuration.port.toString(),
                    login = configuration.login,
                    password = "",
                    mode = configuration.mode,
                )
            } ?: SetupDraft()
        return SetupState(
            draft = draft,
            validation = SetupValidator.validate(draft, limits),
            prerequisites = prerequisites,
            saved = saved,
            limits = limits,
        )
    }

    fun edit(
        state: SetupState,
        draft: SetupDraft,
    ): SetupState =
        state.copy(
            draft = draft,
            validation = SetupValidator.validate(draft, state.limits),
        )

    fun submit(state: SetupState): SetupConfiguration? {
        val draft = state.draft
        val validation = SetupValidator.validate(draft, state.limits)
        if (!validation.isValid) return null
        val port = draft.port.toIntOrNull() ?: return null
        return SetupConfiguration(
            host = draft.host,
            port = port,
            login = draft.login,
            mode = draft.mode,
        )
    }
}
