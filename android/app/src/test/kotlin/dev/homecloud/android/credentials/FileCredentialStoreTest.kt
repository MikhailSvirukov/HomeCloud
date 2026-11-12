package dev.homecloud.android.credentials

import dev.homecloud.android.identity.AesGcmSealer
import dev.homecloud.android.identity.IdentityStorageException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.nio.ByteBuffer
import java.util.Base64
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * Asserts the persistence rules of `FR-AND-022 / 1`, `FR-AND-024 / 1` and `FR-AND-025 / 1`: the
 * credential survives a restart, the file holds only an encrypted `payload=` envelope without the
 * login or password, malformed content and an unavailable key are reported as
 * [CredentialStorageException] while the stored file stays intact, a later save replaces the first
 * credential, clearing is repeatable, and an incomplete credential is rejected before writing.
 *
 * The cipher uses [AesGcmSealer] with an in-test AES key, so the store runs on the plain JVM
 * without the Android KeyStore provider that Robolectric does not ship.
 */
class FileCredentialStoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun `save then load returns the stored credential`() {
        val store = store()

        store.save(StoredCredential(login = LOGIN, password = PASSWORD))

        assertEquals(StoredCredential(login = LOGIN, password = PASSWORD), store.load())
    }

    @Test
    fun `a fresh store over the same file reads the saved credential`() {
        val file = credentialFile()
        val key = aesKey()
        FileCredentialStore(file, AesGcmCredentialCipher(key))
            .save(StoredCredential(login = LOGIN, password = PASSWORD))

        val restored = FileCredentialStore(file, AesGcmCredentialCipher(key)).load()

        assertEquals(StoredCredential(login = LOGIN, password = PASSWORD), restored)
    }

    @Test
    fun `an envelope with invalid utf8 is rejected`() {
        val file = credentialFile()
        val cipher = AesGcmCredentialCipher(aesKey())
        val invalidLogin = byteArrayOf(0xC3.toByte(), 0x28)
        val password = PASSWORD.toByteArray(Charsets.UTF_8)
        val payload =
            ByteBuffer
                .allocate(LOGIN_LENGTH_BYTES + invalidLogin.size + password.size)
                .putInt(invalidLogin.size)
                .put(invalidLogin)
                .put(password)
                .array()
        writeEnvelope(file, cipher, payload)

        assertThrows(CredentialStorageException::class.java) {
            FileCredentialStore(file, cipher).load()
        }
    }

    @Test
    fun `the credential value does not print the password`() {
        val text = StoredCredential(login = LOGIN, password = PASSWORD).toString()

        assertFalse(text.contains(PASSWORD))
        assertFalse(text.contains(LOGIN))
    }

    @Test
    fun `save writes one encrypted payload line without the credential`() {
        val file = credentialFile()

        store(file).save(StoredCredential(login = LOGIN, password = PASSWORD))

        val content = file.readText(Charsets.UTF_8)
        assertEquals(1, content.lines().count { line -> line.startsWith(PAYLOAD_PREFIX) })
        assertTrue(content.endsWith(NEWLINE))
        assertFalse(content.contains(LOGIN))
        assertFalse(content.contains(PASSWORD))
    }

    @Test
    fun `save creates a missing directory`() {
        val file = File(File(temporaryFolder.root, NESTED_DIRECTORY), FILE_NAME)
        val store = FileCredentialStore(file, AesGcmCredentialCipher(aesKey()))

        store.save(StoredCredential(login = LOGIN, password = PASSWORD))

        assertEquals(StoredCredential(login = LOGIN, password = PASSWORD), store.load())
    }

    @Test
    fun `missing file loads no credential`() {
        assertNull(store().load())
    }

    @Test
    fun `corrupt line throws`() {
        val file = storeFile(CORRUPT_LINE + NEWLINE)

        assertThrows(CredentialStorageException::class.java) { store(file).load() }
    }

    @Test
    fun `bad base64 throws`() {
        val file = storeFile(PAYLOAD_PREFIX + INVALID_BASE64 + NEWLINE)

        assertThrows(CredentialStorageException::class.java) { store(file).load() }
    }

    @Test
    fun `short decrypted payload throws`() {
        val cipher = AesGcmCredentialCipher(aesKey())
        val file = credentialFile()
        writeEnvelope(file, cipher, ByteArray(SHORT_PAYLOAD_BYTES))

        assertThrows(CredentialStorageException::class.java) { FileCredentialStore(file, cipher).load() }
    }

    @Test
    fun `decrypt failure throws and keeps the stored file intact`() {
        val file = credentialFile()
        store(file).save(StoredCredential(login = LOGIN, password = PASSWORD))
        val before = file.readBytes()
        val failingStore = FileCredentialStore(file, AesGcmCredentialCipher(aesKey(), decryptFailure = true))

        val failure = assertThrows(CredentialStorageException::class.java) { failingStore.load() }

        assertArrayEquals(before, file.readBytes())
        assertFalse(failure.message.orEmpty().contains(LOGIN))
        assertFalse(failure.message.orEmpty().contains(PASSWORD))
        assertFalse(failure.message.orEmpty().contains(file.path))
    }

    @Test
    fun `second save replaces the first credential`() {
        val file = credentialFile()
        val store = store(file)
        store.save(StoredCredential(login = FIRST_LOGIN, password = FIRST_PASSWORD))

        store.save(StoredCredential(login = LOGIN, password = PASSWORD))

        assertEquals(StoredCredential(login = LOGIN, password = PASSWORD), store.load())
        assertFalse(file.readText(Charsets.UTF_8).contains(FIRST_LOGIN))
    }

    @Test
    fun `clear removes the file and a second clear is harmless`() {
        val file = credentialFile()
        val store = store(file)
        store.save(StoredCredential(login = LOGIN, password = PASSWORD))

        store.clear()
        store.clear()

        assertFalse(file.exists())
    }

    @Test
    fun `empty login is rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            store().save(StoredCredential(login = EMPTY, password = PASSWORD))
        }
    }

    @Test
    fun `empty password is rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            store().save(StoredCredential(login = LOGIN, password = EMPTY))
        }
    }

    private fun credentialFile(): File = File(temporaryFolder.newFolder(), FILE_NAME)

    private fun store(file: File = credentialFile()): FileCredentialStore = FileCredentialStore(file, AesGcmCredentialCipher(aesKey()))

    private fun storeFile(content: String): File {
        val file = credentialFile()
        file.writeText(content, Charsets.UTF_8)
        return file
    }

    private fun writeEnvelope(
        file: File,
        cipher: CredentialCipher,
        payload: ByteArray,
    ) {
        val encrypted = cipher.encrypt(payload)
        file.writeText(PAYLOAD_PREFIX + BASE64_ENCODER.encodeToString(encrypted) + NEWLINE, Charsets.UTF_8)
    }

    private fun aesKey(): SecretKey =
        KeyGenerator
            .getInstance(AES_ALGORITHM)
            .apply { init(KEY_BITS) }
            .generateKey()

    private companion object {
        const val FILE_NAME = "account-credential"
        const val NESTED_DIRECTORY = "nested/credentials"
        const val NEWLINE = "\n"
        const val EMPTY = ""
        const val PAYLOAD_PREFIX = "payload="
        const val LOGIN_LENGTH_BYTES = 4

        const val LOGIN = "user@example.test"
        const val PASSWORD = "correct horse battery staple"
        const val FIRST_LOGIN = "other@example.test"
        const val FIRST_PASSWORD = "an earlier password"

        const val CORRUPT_LINE = "not-a-payload-line"
        const val INVALID_BASE64 = "not-base64"
        const val SHORT_PAYLOAD_BYTES = 2

        const val AES_ALGORITHM = "AES"
        const val KEY_BITS = 256

        val BASE64_ENCODER = Base64.getEncoder()
    }
}

/**
 * Reversible [CredentialCipher] over [AesGcmSealer] with a caller-supplied key.
 *
 * [decryptFailure] makes opening raise the [CredentialStorageException] the store must not turn
 * into absence while the caller is asked for the password again.
 */
private class AesGcmCredentialCipher(
    private val key: SecretKey,
    private val decryptFailure: Boolean = false,
) : CredentialCipher {
    override fun encrypt(plaintext: ByteArray): ByteArray = AesGcmSealer.seal(key, plaintext)

    override fun decrypt(payload: ByteArray): ByteArray {
        if (decryptFailure) throw CredentialStorageException(DECRYPT_FAILURE_MESSAGE)
        return try {
            AesGcmSealer.open(key, payload)
        } catch (exception: IdentityStorageException) {
            throw CredentialStorageException(DECRYPT_FAILURE_MESSAGE, exception)
        }
    }

    private companion object {
        const val DECRYPT_FAILURE_MESSAGE = "test cipher cannot decrypt"
    }
}
