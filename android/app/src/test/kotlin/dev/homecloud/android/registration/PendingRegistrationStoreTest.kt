package dev.homecloud.android.registration

import dev.homecloud.android.identity.IdentityCipher
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
import java.util.Base64

/**
 * Asserts the durable pending-registration envelope of `FR-AND-053 / 1`, `FR-AND-054 / 1` and
 * `FR-AND-055 / 1`: every field survives a load, the password and key material stay inside the
 * encrypted `payload=` envelope, unrecoverable content is reported as no record instead of a
 * partial attempt, a record that cannot form the text format is rejected before writing, and a
 * later save replaces the file.
 *
 * The cipher is a deterministic test double so the envelope shape and the store's failure handling
 * are exercised without the Android KeyStore.
 */
class PendingRegistrationStoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    private val cipher = FakeIdentityCipher()

    @Test
    fun `save then load round trips every field`() {
        val store = store()
        val record = record()

        store.save(record)

        val loaded = requireValue(store.load(), "pending registration was not loaded")
        assertArrayEquals(record.requestId, loaded.requestId)
        assertEquals(record.login, loaded.login)
        assertEquals(record.password, loaded.password)
        assertArrayEquals(record.x25519PublicKey, loaded.x25519PublicKey)
        assertArrayEquals(record.ed25519PublicKey, loaded.ed25519PublicKey)
        assertArrayEquals(record.arguments, loaded.arguments)
        assertEquals(record.startedAtMillis, loaded.startedAtMillis)
    }

    @Test
    fun `save writes one encrypted payload line without plaintext secrets`() {
        val file = pendingFile()
        val record = record()

        FilePendingRegistrationStore(file, cipher).save(record)

        val content = file.readText(Charsets.UTF_8)
        assertTrue(content.startsWith(PAYLOAD_PREFIX))
        assertFalse(content.contains(record.login))
        assertFalse(content.contains(record.password))
        listOf(record.x25519PublicKey, record.ed25519PublicKey, record.arguments).forEach { value ->
            assertFalse(content.contains(BASE64_ENCODER.encodeToString(value)))
        }
    }

    @Test
    fun `missing file loads no record`() {
        assertNull(store().load())
    }

    @Test
    fun `invalid envelope loads no record`() {
        val file = pendingFile()
        file.writeText(INVALID_ENVELOPE, Charsets.UTF_8)

        assertNull(FilePendingRegistrationStore(file, cipher).load())
    }

    @Test
    fun `cipher failure loads no record`() {
        val file = pendingFile()
        file.writeText(envelope(formatLines(record()).joinToString(NEWLINE)), Charsets.UTF_8)
        val store = FilePendingRegistrationStore(file, FakeIdentityCipher(decryptFailure = true))

        assertNull(store.load())
    }

    @Test
    fun `wrong-length request id loads no record`() {
        val malformed = record().copy(requestId = ByteArray(REQUEST_ID_BYTES - 1))

        assertNull(storeWithRecord(malformed).load())
    }

    @Test
    fun `wrong-length public key loads no record`() {
        val malformed = record().copy(x25519PublicKey = ByteArray(PUBLIC_KEY_BYTES + 1))

        assertNull(storeWithRecord(malformed).load())
    }

    @Test
    fun `empty login loads no record`() {
        val malformed = record().copy(login = "")

        assertNull(storeWithRecord(malformed).load())
    }

    @Test
    fun `empty password loads no record`() {
        val malformed = record().copy(password = "")

        assertNull(storeWithRecord(malformed).load())
    }

    @Test
    fun `save rejects a request id of the wrong length`() {
        assertSaveRejects(record().copy(requestId = ByteArray(REQUEST_ID_BYTES + 1)))
    }

    @Test
    fun `save rejects a public key of the wrong length`() {
        assertSaveRejects(record().copy(x25519PublicKey = ByteArray(PUBLIC_KEY_BYTES - 1)))
        assertSaveRejects(record().copy(ed25519PublicKey = ByteArray(PUBLIC_KEY_BYTES + 1)))
    }

    @Test
    fun `save rejects an empty login`() {
        assertSaveRejects(record().copy(login = ""))
    }

    @Test
    fun `save rejects an empty password`() {
        assertSaveRejects(record().copy(password = ""))
    }

    @Test
    fun `clear removes the file and succeeds when nothing is stored`() {
        val file = pendingFile()
        val store = FilePendingRegistrationStore(file, cipher)
        store.save(record())

        store.clear()
        store.clear()

        assertFalse(file.exists())
    }

    @Test
    fun `second save replaces the first record`() {
        val directory = temporaryFolder.newFolder()
        val file = File(directory, FILE_NAME)
        val store = FilePendingRegistrationStore(file, cipher)
        val replacement =
            record().copy(
                requestId = ByteArray(REQUEST_ID_BYTES) { index -> (index + 101).toByte() },
                login = REPLACEMENT_LOGIN,
                startedAtMillis = STARTED_AT_MILLIS + 1,
            )

        store.save(record())
        store.save(replacement)

        val loaded = requireValue(store.load(), "pending registration was not loaded")
        assertArrayEquals(replacement.requestId, loaded.requestId)
        assertEquals(replacement.login, loaded.login)
        assertEquals(replacement.startedAtMillis, loaded.startedAtMillis)
        assertEquals(
            listOf(FILE_NAME),
            directory
                .listFiles()
                .orEmpty()
                .map { entry -> entry.name }
                .sorted(),
        )
    }

    @Test
    fun `toString hides the password`() {
        assertFalse(record().toString().contains(PASSWORD))
    }

    @Test
    fun `toString hides the key bytes`() {
        val text = record().toString()

        assertFalse(text.contains(BASE64_ENCODER.encodeToString(x25519PublicKey)))
        assertFalse(text.contains(BASE64_ENCODER.encodeToString(ed25519PublicKey)))
    }

    private fun pendingFile(): File = File(temporaryFolder.newFolder(), FILE_NAME)

    private fun store(): FilePendingRegistrationStore = FilePendingRegistrationStore(pendingFile(), cipher)

    private fun storeWithRecord(record: PendingRegistration): FilePendingRegistrationStore {
        val file = pendingFile()
        file.writeText(envelope(formatLines(record).joinToString(NEWLINE)), Charsets.UTF_8)
        return FilePendingRegistrationStore(file, cipher)
    }

    private fun envelope(plaintext: String): String =
        PAYLOAD_PREFIX + BASE64_ENCODER.encodeToString(cipher.encrypt(plaintext.toByteArray(Charsets.UTF_8))) + NEWLINE

    private fun record(): PendingRegistration =
        PendingRegistration(
            requestId = requestId,
            login = LOGIN,
            password = PASSWORD,
            x25519PublicKey = x25519PublicKey,
            ed25519PublicKey = ed25519PublicKey,
            arguments = arguments,
            startedAtMillis = STARTED_AT_MILLIS,
        )

    private fun formatLines(record: PendingRegistration): List<String> =
        listOf(
            line(KEY_REQUEST_ID, record.requestId),
            "$KEY_LOGIN$KEY_SEPARATOR${record.login}",
            line(KEY_PASSWORD, record.password.toByteArray(Charsets.UTF_8)),
            line(KEY_X25519_PUBLIC_KEY, record.x25519PublicKey),
            line(KEY_ED25519_PUBLIC_KEY, record.ed25519PublicKey),
            line(KEY_ARGUMENTS, record.arguments),
            "$KEY_STARTED_AT_MILLIS$KEY_SEPARATOR${record.startedAtMillis}",
        )

    private fun line(
        key: String,
        value: ByteArray,
    ): String = "$key$KEY_SEPARATOR${BASE64_ENCODER.encodeToString(value)}"

    private fun assertSaveRejects(record: PendingRegistration) {
        assertThrows(IllegalArgumentException::class.java) { store().save(record) }
    }

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private val requestId = ByteArray(REQUEST_ID_BYTES) { index -> (index + 1).toByte() }
    private val x25519PublicKey = ByteArray(PUBLIC_KEY_BYTES) { index -> (index + 31).toByte() }
    private val ed25519PublicKey = ByteArray(PUBLIC_KEY_BYTES) { index -> (index + 61).toByte() }
    private val arguments = RegistrationArguments.encode(x25519PublicKey, ed25519PublicKey)

    private companion object {
        const val FILE_NAME = "pending-registration"
        const val NEWLINE = "\n"
        const val KEY_SEPARATOR = "="
        const val PAYLOAD_PREFIX = "payload="

        const val REQUEST_ID_BYTES = 16
        const val PUBLIC_KEY_BYTES = 32
        const val STARTED_AT_MILLIS = 1_700_000_000_000L

        const val LOGIN = "pending-login"
        const val REPLACEMENT_LOGIN = "replacement-login"
        const val PASSWORD = "pending-password"

        const val KEY_REQUEST_ID = "requestId"
        const val KEY_LOGIN = "login"
        const val KEY_PASSWORD = "password"
        const val KEY_X25519_PUBLIC_KEY = "x25519PublicKey"
        const val KEY_ED25519_PUBLIC_KEY = "ed25519PublicKey"
        const val KEY_ARGUMENTS = "arguments"
        const val KEY_STARTED_AT_MILLIS = "startedAtMillis"

        const val INVALID_ENVELOPE = "not a pending registration envelope"

        val BASE64_ENCODER = Base64.getEncoder()
    }
}

/**
 * Reversible, deterministic [IdentityCipher] double.
 *
 * The transform is an XOR with [TRANSFORM_MASK], so a test can build the `payload=` envelope for
 * any plaintext it wants the store to read, and [decryptFailure] makes `decrypt` raise the
 * [IdentityStorageException] the store must turn into absence.
 */
private class FakeIdentityCipher(
    private val decryptFailure: Boolean = false,
) : IdentityCipher {
    override fun encrypt(plaintext: ByteArray): ByteArray = plaintext.map(::transform).toByteArray()

    override fun decrypt(payload: ByteArray): ByteArray {
        if (decryptFailure) throw IdentityStorageException(DECRYPT_FAILURE_MESSAGE)
        return payload.map(::transform).toByteArray()
    }

    private fun transform(value: Byte): Byte = (value.toInt() xor TRANSFORM_MASK).toByte()

    private companion object {
        const val TRANSFORM_MASK = 0x5A
        const val DECRYPT_FAILURE_MESSAGE = "test cipher cannot decrypt"
    }
}
