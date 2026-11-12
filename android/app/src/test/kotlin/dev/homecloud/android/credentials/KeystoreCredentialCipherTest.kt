package dev.homecloud.android.credentials

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test
import java.security.Key
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * Pins the key-lookup rules of [KeystoreCredentialCipher] for `FR-AND-022 / 1` and
 * `FR-AND-024 / 1`: the cipher seals and reopens under the supplied key, asks for the key to be
 * created only while encrypting, requires an existing key while decrypting, randomises every
 * encryption, and reports a missing key or provider failure as [CredentialStorageException].
 *
 * The key provider is a test double so the cipher runs on the plain JVM without the Android
 * KeyStore provider that Robolectric does not ship.
 */
class KeystoreCredentialCipherTest {
    @Test
    fun `encrypt then decrypt returns the original plaintext`() {
        val cipher = KeystoreCredentialCipher(FakeCredentialKeyProvider())

        val decrypted = cipher.decrypt(cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8)))

        assertArrayEquals(PLAINTEXT.toByteArray(Charsets.UTF_8), decrypted)
    }

    @Test
    fun `encrypt asks for the key to be created and decrypt asks without creating`() {
        val provider = FakeCredentialKeyProvider()
        val cipher = KeystoreCredentialCipher(provider)

        cipher.decrypt(cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8)))

        assertEquals(listOf(true, false), provider.createFlags)
    }

    @Test
    fun `two encryptions of the same plaintext differ`() {
        val cipher = KeystoreCredentialCipher(FakeCredentialKeyProvider())
        val plaintext = PLAINTEXT.toByteArray(Charsets.UTF_8)

        val first = cipher.encrypt(plaintext)
        val second = cipher.encrypt(plaintext)

        assertFalse(first.contentEquals(second))
    }

    @Test
    fun `decrypt without an existing key throws`() {
        val payload = KeystoreCredentialCipher(FakeCredentialKeyProvider()).encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8))
        val cipher = KeystoreCredentialCipher(FakeCredentialKeyProvider())

        assertThrows(CredentialStorageException::class.java) { cipher.decrypt(payload) }
    }

    @Test
    fun `provider failure while encrypting throws`() {
        val cipher = KeystoreCredentialCipher(FailingCredentialKeyProvider())

        assertThrows(CredentialStorageException::class.java) {
            cipher.encrypt(PLAINTEXT.toByteArray(Charsets.UTF_8))
        }
    }
}

/**
 * Supplies one lazily generated key and records the creation flag of every lookup.
 *
 * A lookup that may not create the key fails until the first creating lookup has stored one, which
 * mirrors the Android KeyStore behaviour the cipher relies on.
 */
private class FakeCredentialKeyProvider : CredentialKeyProvider {
    val createFlags = mutableListOf<Boolean>()
    private var storedKey: SecretKey? = null

    override fun key(create: Boolean): Key {
        createFlags += create
        val existing = storedKey
        return when {
            existing != null -> existing
            create -> aesKey().also { generated -> storedKey = generated }
            else -> throw CredentialStorageException(KEY_FAILURE_MESSAGE)
        }
    }
}

/** Fails every key lookup, as an unavailable Android KeyStore key does. */
private class FailingCredentialKeyProvider : CredentialKeyProvider {
    override fun key(create: Boolean): Key = throw CredentialStorageException(KEY_FAILURE_MESSAGE)
}

private fun aesKey(): SecretKey =
    KeyGenerator
        .getInstance(AES_ALGORITHM)
        .apply { init(KEY_BITS) }
        .generateKey()

private const val PLAINTEXT = "user@example.test\u0000correct horse battery staple"
private const val AES_ALGORITHM = "AES"
private const val KEY_BITS = 256
private const val KEY_FAILURE_MESSAGE = "test key provider cannot supply a key"
