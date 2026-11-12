package dev.homecloud.android.setup

import dev.homecloud.android.credentials.CredentialStorageException
import dev.homecloud.android.credentials.CredentialStore
import dev.homecloud.android.credentials.StoredCredential
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.coroutines.cancellation.CancellationException

/**
 * Asserts the recording rules of `FR-AND-022 / 1`, `FR-AND-023 / 2` and `FR-AND-024 / 1`: a
 * successful setup submission stores the configured login with the in-memory password, a store
 * rejection is swallowed so the registration outcome is unchanged, the recorder keeps no state
 * between recordings, and the password always comes from the submission rather than the
 * configuration.
 */
class SetupCredentialRecorderTest {
    @Test
    fun `record saves the configuration login and the submitted password`() {
        val store = RecordingCredentialStore()

        val recorded = SetupCredentialRecorder(store).record(SUBMISSION)

        assertTrue(recorded)
        assertEquals(StoredCredential(login = LOGIN, password = PASSWORD), store.saved)
    }

    @Test
    fun `record propagates cancellation instead of reporting a failed save`() {
        val store = RecordingCredentialStore(failure = CancellationException())

        assertThrows(CancellationException::class.java) {
            SetupCredentialRecorder(store).record(SUBMISSION)
        }
    }

    @Test
    fun `record returns false without propagating a store rejection`() {
        val failure = CredentialStorageException("Could not store the account credential")
        val store = RecordingCredentialStore(failure = failure)

        val recorded =
            try {
                SetupCredentialRecorder(store).record(SUBMISSION)
            } catch (_: Exception) {
                throw AssertionError("The store failure escaped instead of returning false")
            }

        assertFalse(recorded)
        assertEquals(1, store.saveCalls)
    }

    @Test
    fun `a second record captures the second credential`() {
        val store = RecordingCredentialStore()
        val recorder = SetupCredentialRecorder(store)
        recorder.record(SUBMISSION)

        recorder.record(SetupSubmission(configuration = CONFIGURATION, password = SECOND_PASSWORD))

        assertEquals(2, store.saveCalls)
        assertEquals(StoredCredential(login = LOGIN, password = SECOND_PASSWORD), store.saved)
    }

    @Test
    fun `record stores the submission password rather than a configuration value`() {
        val firstStore = RecordingCredentialStore()
        val secondStore = RecordingCredentialStore()

        SetupCredentialRecorder(firstStore)
            .record(SetupSubmission(configuration = CONFIGURATION, password = PASSWORD))
        SetupCredentialRecorder(secondStore)
            .record(SetupSubmission(configuration = CONFIGURATION, password = SECOND_PASSWORD))

        assertEquals(PASSWORD, firstStore.saved?.password)
        assertEquals(SECOND_PASSWORD, secondStore.saved?.password)
    }

    private companion object {
        const val HOST = "192.0.2.10"
        const val PORT = 8443
        const val LOGIN = "user@example.test"
        const val PASSWORD = "correct horse battery staple"
        const val SECOND_PASSWORD = "a completely different password"

        val CONFIGURATION =
            SetupConfiguration(
                host = HOST,
                port = PORT,
                login = LOGIN,
                mode = SetupMode.REGISTER,
            )
        val SUBMISSION = SetupSubmission(configuration = CONFIGURATION, password = PASSWORD)
    }
}

/** A [CredentialStore] that records the last save and can reject every save with [failure]. */
private class RecordingCredentialStore(
    private val failure: Exception? = null,
) : CredentialStore {
    var saved: StoredCredential? = null
        private set

    var saveCalls: Int = 0
        private set

    override fun load(): StoredCredential? = saved

    override fun save(credential: StoredCredential) {
        saveCalls += 1
        failure?.let { throw it }
        saved = credential
    }

    override fun clear() {
        saved = null
    }
}
