package dev.homecloud.android.setup

import dev.homecloud.android.credentials.CredentialStore
import dev.homecloud.android.credentials.StoredCredential
import kotlin.coroutines.cancellation.CancellationException

/**
 * Stores the credential of a successful setup registration for later background requests
 * (`FR-AND-022 / 1`, `FR-AND-023 / 2`, `FR-AND-024 / 1`). The saved login comes from the
 * configuration while the password comes only from the in-memory submission. A store that rejects
 * the credential leaves the registration outcome unchanged, because the client asks for the
 * password again when the stored credential cannot be loaded (`FR-AND-024 / 1`); the failure is
 * not logged and never carries the login or password.
 */
internal class SetupCredentialRecorder(
    private val store: CredentialStore,
) {
    fun record(submission: SetupSubmission): Boolean =
        try {
            store.save(
                StoredCredential(
                    login = submission.configuration.login,
                    password = submission.password,
                ),
            )
            true
        } catch (cancellation: CancellationException) {
            throw cancellation
        } catch (_: Exception) {
            false
        }
}
