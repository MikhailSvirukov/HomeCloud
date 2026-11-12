package dev.homecloud.android.credentials

import dev.homecloud.android.identity.AesGcmSealer
import dev.homecloud.android.identity.IdentityStorageException
import dev.homecloud.android.identity.KeystoreAesKeyProvider
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.Key
import java.security.ProviderException
import javax.crypto.SecretKey

/**
 * Seals and opens the stored account credential (`FR-AND-022 / 1`).
 *
 * Sealing produces a fresh authenticated envelope, and opening recovers the credential only for a
 * payload sealed under the same key. Failures are reported as [CredentialStorageException] without
 * exposing the credential or key material.
 */
internal interface CredentialCipher {
    /** Seals the credential with a fresh AES-GCM IV. */
    fun encrypt(plaintext: ByteArray): ByteArray

    /** Reopens a sealed payload, rejecting one that is short or fails authentication. */
    fun decrypt(payload: ByteArray): ByteArray
}

/**
 * Obtains the non-exportable key that protects the stored credential (`FR-AND-022 / 1`).
 *
 * A request that may create the key generates it when absent, while a request that may not create
 * it uses only an existing key. Absence or any other retrieval failure is reported as
 * [CredentialStorageException].
 */
internal fun interface CredentialKeyProvider {
    /** Obtains the credential key, creating it only when requested to. */
    fun key(create: Boolean): Key
}

/**
 * Protects the stored credential with a non-exportable Android KeyStore AES-256-GCM key
 * (`FR-AND-022 / 1`, `FR-AND-024 / 1`).
 *
 * Encryption may create the key on first use, while decryption uses only an existing key, so a
 * missing key cannot turn unreadable stored data into a credential. Key retrieval and the AES-GCM
 * envelope report failures as [CredentialStorageException] without exposing the credential or key
 * material.
 */
internal class KeystoreCredentialCipher(
    private val keyProvider: CredentialKeyProvider = AndroidKeystoreCredentialKeyProvider,
) : CredentialCipher {
    override fun encrypt(plaintext: ByteArray): ByteArray =
        attempt(create = true, failedMessage = ENCRYPTION_FAILED_MESSAGE) { key ->
            AesGcmSealer.seal(key, plaintext)
        }

    override fun decrypt(payload: ByteArray): ByteArray =
        attempt(create = false, failedMessage = DECRYPTION_FAILED_MESSAGE) { key ->
            AesGcmSealer.open(key, payload)
        }

    private fun attempt(
        create: Boolean,
        failedMessage: String,
        operation: (SecretKey) -> ByteArray,
    ): ByteArray =
        try {
            val key =
                keyProvider.key(create) as? SecretKey
                    ?: throw CredentialStorageException(KEY_FAILED_MESSAGE)
            operation(key)
        } catch (exception: IdentityStorageException) {
            throw CredentialStorageException(failedMessage, exception)
        } catch (exception: GeneralSecurityException) {
            throw CredentialStorageException(failedMessage, exception)
        } catch (exception: ProviderException) {
            throw CredentialStorageException(failedMessage, exception)
        } catch (exception: IOException) {
            throw CredentialStorageException(failedMessage, exception)
        }
}

/**
 * The Android KeyStore key that protects the stored credential (`FR-AND-022 / 1`,
 * `FR-AND-024 / 1`).
 *
 * The key lives under a credential alias separate from the installation identity as a
 * non-exportable AES-256 key usable for GCM encryption and decryption without user authentication
 * and with randomised encryption required. Encryption may create the key on first use, while
 * decryption uses only an existing key, and any retrieval or generation failure is reported as
 * [CredentialStorageException].
 */
internal object AndroidKeystoreCredentialKeyProvider : CredentialKeyProvider {
    private val keyProvider = KeystoreAesKeyProvider(KEY_ALIAS, KEY_FAILED_MESSAGE)

    override fun key(create: Boolean): Key =
        try {
            keyProvider.key(create)
        } catch (exception: IdentityStorageException) {
            throw CredentialStorageException(KEY_FAILED_MESSAGE, exception)
        }
}

private const val KEY_ALIAS = "homecloud-account-credential"
private const val KEY_FAILED_MESSAGE = "Could not read the account credential key"
private const val ENCRYPTION_FAILED_MESSAGE = "Could not encrypt the account credential"
private const val DECRYPTION_FAILED_MESSAGE = "Could not decrypt the account credential"
