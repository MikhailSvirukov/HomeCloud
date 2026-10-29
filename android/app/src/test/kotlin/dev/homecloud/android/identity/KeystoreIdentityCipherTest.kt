package dev.homecloud.android.identity

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * Pins the key-lookup rules of [KeystoreIdentityCipher] for `FR-AND-026 / 2`: the cipher seals and
 * reopens under the supplied key, asks for the key to be created only while encrypting, and reports
 * a missing key as [IdentityStorageException] instead of creating one while decrypting.
 *
 * The key provider is a test double so the cipher runs on the plain JVM without the Android
 * KeyStore provider that Robolectric does not ship.
 */
class KeystoreIdentityCipherTest {
    @Test
    fun `encrypt then decrypt returns the original plaintext`() {
        val cipher = KeystoreIdentityCipher(RecordingKeyProvider(aesKey()))

        val decrypted = cipher.decrypt(cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8)))

        assertArrayEquals(PLAINTEXT.toByteArray(Charsets.UTF_8), decrypted)
    }

    @Test
    fun `encrypt asks for the key to be created and decrypt asks without creating`() {
        val provider = RecordingKeyProvider(aesKey())
        val cipher = KeystoreIdentityCipher(provider)

        cipher.decrypt(cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8)))

        assertEquals(listOf(true, false), provider.createFlags)
    }

    @Test
    fun `missing key while decrypting throws`() {
        val payload = KeystoreIdentityCipher(RecordingKeyProvider(aesKey())).encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8))
        val cipher = KeystoreIdentityCipher(FailingKeyProvider(failWhenCreate = false))

        assertThrows(IdentityStorageException::class.java) { cipher.decrypt(payload) }
    }

    @Test
    fun `missing key while encrypting throws`() {
        val cipher = KeystoreIdentityCipher(FailingKeyProvider(failWhenCreate = true))

        assertThrows(IdentityStorageException::class.java) { cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8)) }
    }
}

/**
 * Supplies one fixed key and records the creation flag of every lookup.
 */
private class RecordingKeyProvider(
    private val key: SecretKey,
) : IdentityKeyProvider {
    val createFlags = mutableListOf<Boolean>()

    override fun key(create: Boolean): SecretKey {
        createFlags += create
        return key
    }
}

/**
 * Fails a lookup that matches [failWhenCreate] and otherwise supplies a fresh key.
 */
private class FailingKeyProvider(
    private val failWhenCreate: Boolean,
) : IdentityKeyProvider {
    override fun key(create: Boolean): SecretKey {
        if (create == failWhenCreate) throw IdentityStorageException(KEY_FAILURE_MESSAGE)
        return aesKey()
    }
}

private fun aesKey(): SecretKey =
    KeyGenerator
        .getInstance(AES_ALGORITHM)
        .apply { init(KEY_BITS) }
        .generateKey()

private const val PLAINTEXT = "signingPrivate=test-value"
private const val AES_ALGORITHM = "AES"
private const val KEY_BITS = 256
private const val KEY_FAILURE_MESSAGE = "test key provider cannot supply a key"
