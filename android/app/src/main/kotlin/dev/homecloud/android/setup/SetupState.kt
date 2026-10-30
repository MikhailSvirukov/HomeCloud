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

/** The registration state shown by the setup surface (`FR-AND-042 / 1`, `FR-AND-053 / 1`). */
internal enum class RegistrationStatus { NONE, IN_PROGRESS, PENDING, FAILED }

/**
 * The setup surface state: the current draft, its validation, the backup prerequisites, the values
 * that were last saved, the configured credential bounds used to validate the draft
 * (`CFG-CL-029 / 1`), whether the last save attempt failed (`FR-AND-040 / 2`) and the registration
 * status (`FR-AND-042 / 1`, `FR-AND-053 / 1`). A failed save stays visible until the draft is
 * edited or a later save succeeds. Every edit produces a new immutable state.
 */
internal data class SetupState(
    val draft: SetupDraft,
    val validation: SetupValidation,
    val prerequisites: SetupPrerequisites,
    val saved: SetupConfiguration?,
    val limits: SetupLimits = SetupValidator.defaultLimits,
    val saveFailed: Boolean = false,
    val registration: RegistrationStatus = RegistrationStatus.NONE,
) {
    /** Whether the user may start backup: the draft is valid and both prerequisites are met. */
    val canStartBackup: Boolean
        get() = validation.isValid && prerequisites.isReadyForBackup
}

/**
 * The values a valid submission hands to the registration or connection step
 * (`FR-AND-041 / 2`, `FR-AND-042 / 1`): the configuration that may be persisted and the password
 * that must stay in memory only and never reach storage or a log.
 */
internal data class SetupSubmission(
    val configuration: SetupConfiguration,
    val password: String,
)

/**
 * The setup transitions (`FR-AND-040 / 2`, `FR-AND-041 / 2`, `FR-AND-042 / 1`, `FR-AND-053 / 1`,
 * `CFG-CL-029 / 1`): opening a state from an optional saved configuration and the configured
 * credential bounds, editing the draft, submitting it only once valid, recording a configuration
 * after it was persisted, recording a failed save and following a registration attempt through its
 * in-progress, pending and failed states. Editing, submitting and recording a save revalidate with
 * the bounds already held by the state, so a configuration change never mixes limits within one
 * state. Editing after a started or failed attempt returns to the idle status, while a pending
 * attempt keeps its status because its durable record still exists.
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
            registration = RegistrationStatus.NONE,
        )
    }

    fun edit(
        state: SetupState,
        draft: SetupDraft,
    ): SetupState =
        state.copy(
            draft = draft,
            validation = SetupValidator.validate(draft, state.limits),
            saveFailed = false,
            registration =
                if (state.registration == RegistrationStatus.PENDING) {
                    RegistrationStatus.PENDING
                } else {
                    RegistrationStatus.NONE
                },
        )

    fun submit(state: SetupState): SetupSubmission? {
        val draft = state.draft
        val validation = SetupValidator.validate(draft, state.limits)
        if (!validation.isValid) return null
        val port = draft.port.toIntOrNull() ?: return null
        return SetupSubmission(
            configuration =
                SetupConfiguration(
                    host = draft.host,
                    port = port,
                    login = draft.login,
                    mode = draft.mode,
                ),
            password = draft.password,
        )
    }

    fun saved(
        state: SetupState,
        configuration: SetupConfiguration,
    ): SetupState {
        val draft =
            state.draft.copy(
                host = configuration.host,
                port = configuration.port.toString(),
                login = configuration.login,
                password = "",
                mode = configuration.mode,
            )
        return state.copy(
            draft = draft,
            validation = SetupValidator.validate(draft, state.limits),
            saved = configuration,
            saveFailed = false,
            registration = RegistrationStatus.NONE,
        )
    }

    fun saveFailed(state: SetupState): SetupState =
        state.copy(
            saveFailed = true,
            registration = RegistrationStatus.NONE,
        )

    /** Marks that a valid submission started a registration attempt (`FR-AND-042 / 1`). */
    fun registering(state: SetupState): SetupState =
        state.copy(
            registration = RegistrationStatus.IN_PROGRESS,
            saveFailed = false,
        )

    /**
     * Records a successful registration (`FR-AND-042 / 1`, `FR-AND-053 / 1`): the authenticated
     * prerequisite becomes met so backup may start.
     */
    fun registered(
        state: SetupState,
        configuration: SetupConfiguration,
    ): SetupState =
        saved(state, configuration).copy(
            registration = RegistrationStatus.NONE,
            prerequisites = state.prerequisites.copy(isAuthenticated = true),
        )

    /** Keeps a durable registration attempt visible for a later retry (`FR-AND-054 / 1`). */
    fun registrationPending(state: SetupState): SetupState = state.copy(registration = RegistrationStatus.PENDING)

    /** Reports that the registration attempt failed (`FR-AND-042 / 1`). */
    fun registrationFailed(state: SetupState): SetupState = state.copy(registration = RegistrationStatus.FAILED)
}
