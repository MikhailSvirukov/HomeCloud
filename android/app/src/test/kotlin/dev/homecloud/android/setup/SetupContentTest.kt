package dev.homecloud.android.setup

import androidx.activity.ComponentActivity
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import dev.homecloud.android.R
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/** The API level whose behaviour the rendered setup surface is asserted against. */
private const val SETUP_CONTENT_SDK_INT = 33

/**
 * Renders [SetupContent] on the JVM and asserts what the user sees and what the surface reports for
 * `FR-AND-040 / 2`, `FR-AND-041 / 2`, `FR-AND-042 / 1` and `FR-AND-043 / 1`: the labelled inputs,
 * the masked password, the single mode choice, the inline host error, and the prerequisite text
 * that does not block submission of a valid draft.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [SETUP_CONTENT_SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class SetupContentTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<ComponentActivity>()

    private val draftChanges = mutableListOf<SetupDraft>()
    private var submitCount = 0

    @Test
    fun `setup fields, labels and mode options are rendered`() {
        render(stateFor(validDraft(), unmetPrerequisites()))

        listOf(
            title(),
            hostLabel(),
            portLabel(),
            loginLabel(),
            passwordLabel(),
            modeLabel(),
            registerLabel(),
            connectLabel(),
        ).forEach { text -> composeRule.onNodeWithText(text).performScrollTo().assertIsDisplayed() }
        composeRule.onAllNodes(hasSetTextAction()).assertCountEquals(TEXT_FIELD_COUNT)
    }

    @Test
    fun `password field carries password semantics instead of exposing the raw value`() {
        render(stateFor(validDraft(), unmetPrerequisites()))

        val passwordField =
            composeRule
                .onNode(hasSetTextAction() and SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
                .performScrollTo()
        passwordField.assertIsDisplayed()
        composeRule
            .onAllNodes(hasSetTextAction() and SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
            .assertCountEquals(PASSWORD_FIELD_COUNT)
        composeRule
            .onAllNodes(hasText(PASSWORD) and SemanticsMatcher.keyNotDefined(SemanticsProperties.Password))
            .assertCountEquals(0)
    }

    @Test
    fun `choosing a mode reports the chosen mode`() {
        render(stateFor(validDraft(), unmetPrerequisites()))

        composeRule.onNodeWithText(connectLabel()).performScrollTo().performClick()
        composeRule.onNodeWithText(registerLabel()).performScrollTo().performClick()

        assertEquals(
            listOf(SetupMode.CONNECT, SetupMode.REGISTER),
            draftChanges.map(SetupDraft::mode),
        )
    }

    @Test
    fun `invalid host shows the canonical literal error and disables submit`() {
        render(stateFor(validDraft().copy(host = "example.com"), unmetPrerequisites()))

        composeRule.onNodeWithText(notAnIpLiteralError()).performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText(submitAction()).performScrollTo().assertIsNotEnabled()
    }

    @Test
    fun `valid draft with unmet prerequisites shows them and still submits`() {
        render(stateFor(validDraft(), unmetPrerequisites()))

        composeRule.onNodeWithText(prerequisitesTitle()).performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText(mediaAccessMissing()).performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText(authenticationMissing()).performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText(submitAction()).performScrollTo().assertIsEnabled()
        composeRule.onNodeWithText(submitAction()).performScrollTo().performClick()

        assertEquals(1, submitCount)
    }

    private fun render(initialState: SetupState) {
        val state = mutableStateOf(initialState)
        composeRule.setContent {
            SetupContent(
                state = state.value,
                onDraftChange = { draft ->
                    draftChanges += draft
                    state.value = SetupStateMachine.edit(state.value, draft)
                },
                onSubmit = { submitCount += 1 },
            )
        }
    }

    private fun stateFor(
        draft: SetupDraft,
        prerequisites: SetupPrerequisites,
    ): SetupState =
        SetupStateMachine.edit(
            SetupStateMachine.initialState(saved = null, prerequisites = prerequisites),
            draft,
        )

    private fun unmetPrerequisites(): SetupPrerequisites = SetupPrerequisites(hasMediaAccess = false, isAuthenticated = false)

    private fun validDraft(): SetupDraft =
        SetupDraft(
            host = "192.0.2.10",
            port = "8443",
            login = "user",
            password = PASSWORD,
        )

    private fun string(resource: Int): String = composeRule.activity.getString(resource)

    private fun title(): String = string(R.string.setup_title)

    private fun hostLabel(): String = string(R.string.setup_host_label)

    private fun portLabel(): String = string(R.string.setup_port_label)

    private fun loginLabel(): String = string(R.string.setup_login_label)

    private fun passwordLabel(): String = string(R.string.setup_password_label)

    private fun modeLabel(): String = string(R.string.setup_mode_label)

    private fun registerLabel(): String = string(R.string.setup_mode_register)

    private fun connectLabel(): String = string(R.string.setup_mode_connect)

    private fun submitAction(): String = string(R.string.setup_submit_action)

    private fun prerequisitesTitle(): String = string(R.string.setup_prerequisites_title)

    private fun mediaAccessMissing(): String = string(R.string.setup_prerequisite_media_access_missing)

    private fun authenticationMissing(): String = string(R.string.setup_prerequisite_authentication_missing)

    private fun notAnIpLiteralError(): String = string(R.string.setup_error_not_an_ip_literal)

    private companion object {
        const val TEXT_FIELD_COUNT = 4
        const val PASSWORD_FIELD_COUNT = 1
        const val PASSWORD = "secret"
    }
}
