package dev.homecloud.android.identity

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Signals that the protected key material could not be encrypted, decrypted or read. */
internal class IdentityStorageException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/**
 * Protects the installation identity in an authenticated envelope (`FR-AND-026 / 2`).
 *
 * Encrypting produces a fresh IV followed by the authenticated ciphertext, and decrypting recovers
 * the original plaintext only for a payload sealed under the same key. Failures are reported as
 * [IdentityStorageException] without exposing key material or plaintext.
 */
internal interface IdentityCipher {
    /** Seals the identity material with a fresh AES-GCM IV. */
    fun encrypt(plaintext: ByteArray): ByteArray

    /** Reopens a sealed payload, rejecting one that is short or fails authentication. */
    fun decrypt(payload: ByteArray): ByteArray
}

/**
 * AES-256-GCM envelope over a caller-supplied key (`FR-AND-026 / 2`).
 *
 * A sealed payload is the 12-byte IV followed by the ciphertext with its 128-bit authentication
 * tag. Opening accepts only a payload with that minimum shape and succeeds only when the tag
 * authenticates under the same key, so a modified payload is never returned as plaintext. The
 * envelope is independent of where the key came from, and failures never expose key material or
 * plaintext.
 */
internal object AesGcmSealer {
    /** Seals the plaintext under the supplied key with a fresh AES-GCM IV. */
    fun seal(
        key: SecretKey,
        plaintext: ByteArray,
    ): ByteArray =
        try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key)
            val iv = cipher.iv
            if (iv.size != IV_BYTES) {
                throw IdentityStorageException(ENCRYPTION_FAILED_MESSAGE)
            }
            iv + cipher.doFinal(plaintext)
        } catch (exception: GeneralSecurityException) {
            throw IdentityStorageException(ENCRYPTION_FAILED_MESSAGE, exception)
        }

    /** Reopens a sealed payload under the supplied key, rejecting short or unauthenticated input. */
    fun open(
        key: SecretKey,
        payload: ByteArray,
    ): ByteArray {
        if (payload.size < IV_BYTES + TAG_BYTES) {
            throw IdentityStorageException(DECRYPTION_FAILED_MESSAGE)
        }
        return try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                key,
                GCMParameterSpec(TAG_BITS, payload.copyOfRange(0, IV_BYTES)),
            )
            cipher.doFinal(payload, IV_BYTES, payload.size - IV_BYTES)
        } catch (exception: GeneralSecurityException) {
            throw IdentityStorageException(DECRYPTION_FAILED_MESSAGE, exception)
        }
    }

    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128
    private const val TAG_BYTES = TAG_BITS / 8

    private const val ENCRYPTION_FAILED_MESSAGE = "Could not encrypt the installation identity"
    private const val DECRYPTION_FAILED_MESSAGE = "Could not decrypt the installation identity"
}

/**
 * Supplies the non-exportable key that protects the installation identity (`FR-AND-026 / 2`).
 *
 * A request that may create the key generates it when absent, while a request that may not create
 * it uses only an existing key. Absence or any other retrieval failure is reported as
 * [IdentityStorageException] without exposing key material or plaintext.
 */
internal fun interface IdentityKeyProvider {
    /** Obtains the installation identity key, creating it only when requested to. */
    fun key(create: Boolean): SecretKey
}

/**
 * Protects the installation identity with a non-exportable Android KeyStore key
 * (`FR-AND-026 / 2`).
 *
 * Encryption may create the key on first use, while decryption uses only an existing key, so a
 * missing key cannot turn unreadable stored data into a new identity. Key retrieval and the AES-GCM
 * envelope report failures as [IdentityStorageException] without exposing key material or
 * plaintext.
 */
internal class KeystoreIdentityCipher(
    private val keyProvider: IdentityKeyProvider = AndroidKeystoreKeyProvider,
) : IdentityCipher {
    override fun encrypt(plaintext: ByteArray): ByteArray =
        try {
            AesGcmSealer.seal(keyProvider.key(create = true), plaintext)
        } catch (exception: GeneralSecurityException) {
            throw IdentityStorageException(ENCRYPTION_FAILED_MESSAGE, exception)
        } catch (exception: ProviderException) {
            throw IdentityStorageException(ENCRYPTION_FAILED_MESSAGE, exception)
        } catch (exception: IOException) {
            throw IdentityStorageException(ENCRYPTION_FAILED_MESSAGE, exception)
        }

    override fun decrypt(payload: ByteArray): ByteArray =
        try {
            AesGcmSealer.open(keyProvider.key(create = false), payload)
        } catch (exception: GeneralSecurityException) {
            throw IdentityStorageException(DECRYPTION_FAILED_MESSAGE, exception)
        } catch (exception: ProviderException) {
            throw IdentityStorageException(DECRYPTION_FAILED_MESSAGE, exception)
        } catch (exception: IOException) {
            throw IdentityStorageException(DECRYPTION_FAILED_MESSAGE, exception)
        }

    private companion object {
        const val ENCRYPTION_FAILED_MESSAGE = "Could not encrypt the installation identity"
        const val DECRYPTION_FAILED_MESSAGE = "Could not decrypt the installation identity"
    }
}

/**
 * The Android KeyStore key that protects the installation identity (`FR-AND-026 / 2`).
 *
 * The key lives under the installation-identity alias as a non-exportable AES-256 key usable for
 * GCM encryption and decryption without user authentication and with randomised encryption
 * required. Encryption may create the key on first use, while decryption uses only an existing key,
 * and any retrieval or generation failure is reported as [IdentityStorageException] without
 * exposing key material or plaintext.
 */
internal object AndroidKeystoreKeyProvider : IdentityKeyProvider {
    override fun key(create: Boolean): SecretKey =
        try {
            val existing = existingKey()
            if (existing != null) {
                existing
            } else if (create) {
                generateKey()
            } else {
                throw IdentityStorageException(KEY_FAILED_MESSAGE)
            }
        } catch (exception: GeneralSecurityException) {
            throw IdentityStorageException(KEY_FAILED_MESSAGE, exception)
        } catch (exception: ProviderException) {
            throw IdentityStorageException(KEY_FAILED_MESSAGE, exception)
        } catch (exception: IOException) {
            throw IdentityStorageException(KEY_FAILED_MESSAGE, exception)
        }

    private fun existingKey(): SecretKey? = keyStore().getKey(KEY_ALIAS, null) as? SecretKey

    private fun generateKey(): SecretKey {
        val generator = KeyGenerator.getInstance(KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec
                .Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(KEY_SIZE_BITS)
                .setUserAuthenticationRequired(false)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null, null) }

    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val KEY_ALIAS = "homecloud-installation-identity"
    private const val KEY_ALGORITHM_AES = KeyProperties.KEY_ALGORITHM_AES
    private const val KEY_SIZE_BITS = 256

    private const val KEY_FAILED_MESSAGE = "Could not read the installation identity key"
}
