package dev.homecloud.android.credentials

import android.content.Context
import java.io.File

/**
 * Builds the production credential store over the app's private files (`FR-AND-022 / 1`,
 * `FR-AND-024 / 1`). The store is only wired up here; constructing it touches no storage, so the
 * first read or write performs the encrypted file and key work.
 */
internal object CredentialComposition {
    internal const val CREDENTIAL_FILE_NAME = "account-credential"

    fun store(context: Context): CredentialStore =
        FileCredentialStore(
            File(context.filesDir, CREDENTIAL_FILE_NAME),
            KeystoreCredentialCipher(),
        )
}
