package dev.homecloud.android.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the setup transitions of `FR-AND-040 / 2`, `FR-AND-041 / 2` and `FR-AND-042 / 1`, the
 * backup gate of `FR-AND-043 / 1` and the configured credential bounds of `CFG-CL-029 / 1`: a
 * restored configuration leaves the password to the user, every edit revalidates the draft with the
 * bounds its state holds, submission needs a valid draft, a failed save is marked until the draft
 * changes or a save succeeds, and backup needs both prerequisites.
 */
class SetupStateTest {
    @Test
    fun `initial state from a saved configuration restores the draft without a password`() {
        val state = SetupStateMachine.initialState(SAVED, unmetPrerequisites())

        val expected =
            SetupDraft(
                host = SAVED.host,
                port = SAVED.port.toString(),
                login = SAVED.login,
                password = "",
                mode = SAVED.mode,
            )
        assertEquals(expected, state.draft)
        assertEquals(SAVED, state.saved)
    }

    @Test
    fun `initial state without a saved configuration is empty`() {
        val state = SetupStateMachine.initialState(saved = null, prerequisites = unmetPrerequisites())

        assertEquals(SetupDraft(), state.draft)
        assertNull(state.saved)
        assertFalse(state.validation.isValid)
    }

    @Test
    fun `initial state stores the configured limits`() {
        val state =
            SetupStateMachine.initialState(
                saved = null,
                prerequisites = unmetPrerequisites(),
                limits = CONFIGURED_LIMITS,
            )

        assertEquals(CONFIGURED_LIMITS, state.limits)
    }

    @Test
    fun `edit recomputes validation against the configured limits`() {
        val state =
            SetupStateMachine.initialState(
                saved = null,
                prerequisites = unmetPrerequisites(),
                limits = CONFIGURED_LIMITS,
            )

        val edited =
            SetupStateMachine.edit(
                state,
                DRAFT.copy(
                    login = "u".repeat(CONFIGURED_LIMITS.loginMaxBytes + 1),
                    password = CONFIGURED_PASSWORD,
                ),
            )

        assertEquals(mapOf(SetupField.LOGIN to SetupError.LOGIN_TOO_LONG), edited.validation.errors)
    }

    @Test
    fun `edit replaces the draft and recomputes validation`() {
        val state = SetupStateMachine.initialState(saved = null, prerequisites = unmetPrerequisites())

        val edited = SetupStateMachine.edit(state, DRAFT)

        assertEquals(DRAFT, edited.draft)
        assertTrue(edited.validation.errors.toString(), edited.validation.isValid)
    }

    @Test
    fun `submit of a valid draft returns the configuration and the entered password`() {
        val state = SetupStateMachine.edit(initialState(), DRAFT)

        val submission = SetupStateMachine.submit(state)

        assertEquals(SetupSubmission(configuration = CONFIGURATION, password = PASSWORD), submission)
    }

    @Test
    fun `submit of an invalid draft returns nothing`() {
        val state = SetupStateMachine.edit(initialState(), DRAFT.copy(host = "example.com"))

        assertNull(SetupStateMachine.submit(state))
    }

    @Test
    fun `saved records the configuration and refills the draft without a password`() {
        val state = SetupStateMachine.edit(initialState(), DRAFT)

        val savedState = SetupStateMachine.saved(state, SAVED)

        val expectedDraft =
            SetupDraft(
                host = SAVED.host,
                port = SAVED.port.toString(),
                login = SAVED.login,
                password = "",
                mode = SAVED.mode,
            )
        assertEquals(expectedDraft, savedState.draft)
        assertEquals(SAVED, savedState.saved)
        assertEquals(mapOf(SetupField.PASSWORD to SetupError.REQUIRED), savedState.validation.errors)
    }

    @Test
    fun `initial state does not mark a save failure`() {
        val state = initialState()

        assertFalse(state.saveFailed)
    }

    @Test
    fun `save failure is marked without changing the rest of the state`() {
        val state =
            SetupStateMachine.edit(
                SetupStateMachine.initialState(
                    saved = SAVED,
                    prerequisites = SetupPrerequisites(hasMediaAccess = true, isAuthenticated = true),
                    limits = CONFIGURED_LIMITS,
                ),
                DRAFT,
            )

        val failedState = SetupStateMachine.saveFailed(state)

        assertTrue(failedState.saveFailed)
        assertEquals(state.draft, failedState.draft)
        assertEquals(state.validation, failedState.validation)
        assertEquals(state.prerequisites, failedState.prerequisites)
        assertEquals(state.saved, failedState.saved)
        assertEquals(state.limits, failedState.limits)
    }

    @Test
    fun `edit clears a marked save failure`() {
        val failedState = SetupStateMachine.saveFailed(SetupStateMachine.edit(initialState(), DRAFT))

        val edited = SetupStateMachine.edit(failedState, DRAFT.copy(login = "other"))

        assertFalse(edited.saveFailed)
    }

    @Test
    fun `saved clears a marked save failure`() {
        val failedState = SetupStateMachine.saveFailed(SetupStateMachine.edit(initialState(), DRAFT))

        val savedState = SetupStateMachine.saved(failedState, SAVED)

        assertFalse(savedState.saveFailed)
    }

    @Test
    fun `backup can start when the draft is valid and both prerequisites are met`() {
        val state = stateWith(DRAFT, hasMediaAccess = true, isAuthenticated = true)

        assertTrue(state.canStartBackup)
    }

    @Test
    fun `backup cannot start when the draft is invalid`() {
        val state =
            stateWith(
                DRAFT.copy(host = "example.com"),
                hasMediaAccess = true,
                isAuthenticated = true,
            )

        assertFalse(state.canStartBackup)
    }

    @Test
    fun `backup cannot start without media access`() {
        val state = stateWith(DRAFT, hasMediaAccess = false, isAuthenticated = true)

        assertFalse(state.canStartBackup)
    }

    @Test
    fun `backup cannot start without authentication`() {
        val state = stateWith(DRAFT, hasMediaAccess = true, isAuthenticated = false)

        assertFalse(state.canStartBackup)
    }

    private fun initialState(): SetupState = SetupStateMachine.initialState(saved = null, prerequisites = unmetPrerequisites())

    private fun stateWith(
        draft: SetupDraft,
        hasMediaAccess: Boolean,
        isAuthenticated: Boolean,
    ): SetupState {
        val state =
            SetupStateMachine.initialState(
                saved = null,
                prerequisites = SetupPrerequisites(hasMediaAccess, isAuthenticated),
            )
        return SetupStateMachine.edit(state, draft)
    }

    private fun unmetPrerequisites(): SetupPrerequisites = SetupPrerequisites(hasMediaAccess = false, isAuthenticated = false)

    private companion object {
        const val HOST = "192.0.2.10"
        const val PORT_TEXT = "8443"
        const val PORT = 8443
        const val LOGIN = "user"
        const val PASSWORD = "secret"

        /** A password inside [CONFIGURED_LIMITS], so only the login carries an error. */
        const val CONFIGURED_PASSWORD = "pass"

        val DRAFT = SetupDraft(host = HOST, port = PORT_TEXT, login = LOGIN, password = PASSWORD)
        val CONFIGURATION = SetupConfiguration(host = HOST, port = PORT, login = LOGIN, mode = SetupMode.REGISTER)
        val SAVED = SetupConfiguration(host = "2001:db8::1", port = PORT, login = "saved-user", mode = SetupMode.CONNECT)

        val CONFIGURED_LIMITS =
            SetupLimits(
                loginMinBytes = 2,
                loginMaxBytes = 4,
                passwordMinBytes = 2,
                passwordMaxBytes = 4,
            )
    }
}
