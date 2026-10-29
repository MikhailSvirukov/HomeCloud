package dev.homecloud.android.identity

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.crypto.X25519KeyPair
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.util.Base64

/**
 * Asserts the persistence rules of `FR-DED-022 / 1`, `INT-NET-008 / 2`, `INT-NET-025 / 1`,
 * `INT-NET-034 / 1` and `FR-AND-026 / 2`: the identity survives a restart, the optional sender id
 * and dedicated server keys are preserved, the file holds only an encrypted `payload=` envelope,
 * malformed envelopes, non-canonical Base64 and cipher failures are reported as absence, an
 * identity that cannot form the text format is rejected before writing, and a later save replaces
 * the file.
 *
 * The cipher is a test double so the envelope shape and the store's failure handling are exercised
 * without the Android KeyStore.
 */
class FileInstallationIdentityStoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    private val cipher = FakeIdentityCipher()

    @Test
    fun `round trip without optional values preserves both key pairs`() {
        val store = store()
        val identity = InstallationIdentityFactory.newIdentity()

        store.save(identity)

        assertIdentityEquals(identity, store.load())
    }

    @Test
    fun `round trip with sender id and dedicated server keys preserves them`() {
        val store = store()
        val identity = identityWithOptionalValues()

        store.save(identity)

        assertIdentityEquals(identity, store.load())
    }

    @Test
    fun `fresh store over the same file loads the saved identity`() {
        val file = identityFile()
        val identity = InstallationIdentityFactory.newIdentity()
        store(file).save(identity)

        assertIdentityEquals(identity, store(file).load())
    }

    @Test
    fun `missing file loads no identity`() {
        assertNull(store().load())
    }

    @Test
    fun `missing required line loads no identity`() {
        val lines = validLines().filterNot { line -> line.startsWith("$KEY_AGREEMENT_PUBLIC$KEY_SEPARATOR") }

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `unknown key loads no identity`() {
        val lines = validLines() + formatLine(UNKNOWN_KEY, ByteArray(KEY_BYTES))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `duplicated key loads no identity`() {
        val lines = validLines()

        assertNull(storeWithLines(lines + lines.first()).load())
    }

    @Test
    fun `invalid base64 loads no identity`() {
        val lines = validLines().withRawValue(KEY_SIGNING_PRIVATE, INVALID_BASE64)

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `wrong-length private key loads no identity`() {
        val lines = validLines().withEncodedValue(KEY_SIGNING_PRIVATE, ByteArray(KEY_BYTES - 1))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `wrong-length public key loads no identity`() {
        val lines = validLines().withEncodedValue(KEY_AGREEMENT_PUBLIC, ByteArray(KEY_BYTES + 1))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `signing public key that does not match the private key loads no identity`() {
        val signing = Ed25519.generate()
        val otherSigning = Ed25519.generate()
        val lines = validLines(signing = signing).withEncodedValue(KEY_SIGNING_PUBLIC, otherSigning.publicKey)

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `agreement public key that does not match the private key loads no identity`() {
        val agreement = X25519.generate()
        val otherAgreement = X25519.generate()
        val lines = validLines(agreement = agreement).withEncodedValue(KEY_AGREEMENT_PUBLIC, otherAgreement.publicKey)

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `sender id of the wrong length loads no identity`() {
        val lines = validLines(senderId = ByteArray(SENDER_ID_BYTES - 1))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `dedicated server key of the wrong length loads no identity`() {
        val lines =
            validLines(
                serverX25519 = ByteArray(KEY_BYTES - 1),
                serverEd25519 = ByteArray(KEY_BYTES),
            )

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `one dedicated server key without the other loads no identity`() {
        val lines = validLines(serverX25519 = ByteArray(KEY_BYTES))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `sender id without dedicated server keys loads no identity`() {
        val lines = validLines(senderId = ByteArray(SENDER_ID_BYTES) { index -> (index + 1).toByte() })

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `one trailing newline in the text format is tolerated`() {
        val plaintext = validLines().joinToString(NEWLINE) + NEWLINE

        assertNotNull(storeWithPlaintext(plaintext).load())
    }

    @Test
    fun `two trailing newlines in the text format load no identity`() {
        val plaintext = validLines().joinToString(NEWLINE) + NEWLINE + NEWLINE

        assertNull(storeWithPlaintext(plaintext).load())
    }

    @Test
    fun `unpadded base64 text value loads no identity`() {
        val signing = Ed25519.generate()
        val encoded = BASE64_ENCODER.encodeToString(signing.privateKey)
        val lines = validLines(signing = signing).withRawValue(KEY_SIGNING_PRIVATE, encoded.trimEnd(BASE64_PADDING))

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `extra-padded base64 text value loads no identity`() {
        val signing = Ed25519.generate()
        val encoded = BASE64_ENCODER.encodeToString(signing.privateKey)
        val lines = validLines(signing = signing).withRawValue(KEY_SIGNING_PRIVATE, encoded + BASE64_PADDING)

        assertNull(storeWithLines(lines).load())
    }

    @Test
    fun `envelope without a trailing newline is tolerated`() {
        val content = envelope(validLines().joinToString(NEWLINE)).removeSuffix(NEWLINE)

        assertNotNull(storeWithEnvelope(content).load())
    }

    @Test
    fun `extra blank line loads no identity`() {
        val content = envelope(validLines().joinToString(NEWLINE))

        assertNull(storeWithEnvelope(content + NEWLINE).load())
    }

    @Test
    fun `missing payload line loads no identity`() {
        assertNull(storeWithEnvelope(validLines().joinToString(NEWLINE)).load())
    }

    @Test
    fun `duplicated payload line loads no identity`() {
        val content = envelope(validLines().joinToString(NEWLINE))

        assertNull(storeWithEnvelope(content + content).load())
    }

    @Test
    fun `invalid base64 payload loads no identity`() {
        assertNull(storeWithEnvelope("$PAYLOAD_PREFIX$INVALID_BASE64$NEWLINE").load())
    }

    @Test
    fun `unpadded base64 payload loads no identity`() {
        val payload = encodePayload(validLines().joinToString(NEWLINE))

        assertNull(storeWithEnvelope(envelopeWithEncodedPayload(payload.trimEnd(BASE64_PADDING))).load())
    }

    @Test
    fun `extra-padded base64 payload loads no identity`() {
        val payload = encodePayload(validLines().joinToString(NEWLINE))

        assertNull(storeWithEnvelope(envelopeWithEncodedPayload(payload + BASE64_PADDING)).load())
    }

    @Test
    fun `empty payload loads no identity`() {
        assertNull(storeWithEnvelope("$PAYLOAD_PREFIX$NEWLINE").load())
    }

    @Test
    fun `one extra envelope line loads no identity`() {
        val content = envelope(validLines().joinToString(NEWLINE))

        assertNull(storeWithEnvelope(content + "$UNKNOWN_KEY$KEY_SEPARATOR$NEWLINE").load())
    }

    @Test
    fun `cipher failure loads no identity`() {
        val file = identityFile()
        file.writeText(envelope(validLines().joinToString(NEWLINE)), Charsets.UTF_8)
        val failingStore = FileInstallationIdentityStore(file, FakeIdentityCipher(decryptFailure = true))

        assertNull(failingStore.load())
    }

    @Test
    fun `save rejects a signing private key of the wrong length`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                signingKeyPair =
                    Ed25519KeyPair(
                        privateKey = ByteArray(KEY_BYTES - 1),
                        publicKey = identity.signingKeyPair.publicKey,
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects a signing public key of the wrong length`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                signingKeyPair =
                    Ed25519KeyPair(
                        privateKey = identity.signingKeyPair.privateKey,
                        publicKey = ByteArray(KEY_BYTES + 1),
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects an agreement private key of the wrong length`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                agreementKeyPair =
                    X25519KeyPair(
                        privateKey = ByteArray(KEY_BYTES - 1),
                        publicKey = identity.agreementKeyPair.publicKey,
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects an agreement public key of the wrong length`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                agreementKeyPair =
                    X25519KeyPair(
                        privateKey = identity.agreementKeyPair.privateKey,
                        publicKey = ByteArray(KEY_BYTES + 1),
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects a signing public key that does not match its private key`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                signingKeyPair =
                    Ed25519KeyPair(
                        privateKey = identity.signingKeyPair.privateKey,
                        publicKey = Ed25519.generate().publicKey,
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects an agreement public key that does not match its private key`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val malformed =
            identity.copy(
                agreementKeyPair =
                    X25519KeyPair(
                        privateKey = identity.agreementKeyPair.privateKey,
                        publicKey = X25519.generate().publicKey,
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects a sender id of the wrong length`() {
        val malformed = identityWithOptionalValues().copy(senderId = ByteArray(SENDER_ID_BYTES - 1))

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects a sender id without dedicated server keys`() {
        val malformed = identityWithOptionalValues().copy(dedicatedServerKeys = null)

        assertSaveRejects(malformed)
    }

    @Test
    fun `save rejects a dedicated server key of the wrong length`() {
        val malformed =
            identityWithOptionalValues().copy(
                dedicatedServerKeys =
                    DedicatedServerKeys(
                        x25519PublicKey = ByteArray(KEY_BYTES - 1),
                        ed25519PublicKey = ByteArray(KEY_BYTES),
                    ),
            )

        assertSaveRejects(malformed)
    }

    @Test
    fun `save writes one encrypted payload line without private keys`() {
        val file = identityFile()
        val identity = identityWithOptionalValues()

        store(file).save(identity)

        val content = file.readText(Charsets.UTF_8)
        val payloadLines = content.lines().filter { line -> line.startsWith(PAYLOAD_PREFIX) }
        assertEquals(1, payloadLines.size)
        assertFalse(content.contains(KEY_SIGNING_PRIVATE))
        assertFalse(content.contains(KEY_AGREEMENT_PRIVATE))
        assertFalse(content.contains(BASE64_ENCODER.encodeToString(identity.signingKeyPair.privateKey)))
        assertFalse(content.contains(BASE64_ENCODER.encodeToString(identity.agreementKeyPair.privateKey)))
    }

    @Test
    fun `second save replaces the first identity and leaves no temporary file`() {
        val directory = temporaryFolder.newFolder()
        val file = File(directory, FILE_NAME)
        val store = store(file)
        store.save(InstallationIdentityFactory.newIdentity())
        val replacement = identityWithOptionalValues()

        store.save(replacement)

        assertIdentityEquals(replacement, store.load())
        assertEquals(
            listOf(FILE_NAME),
            directory
                .listFiles()
                .orEmpty()
                .map { entry -> entry.name }
                .sorted(),
        )
    }

    private fun identityFile(): File = File(temporaryFolder.newFolder(), FILE_NAME)

    private fun store(file: File = identityFile()): FileInstallationIdentityStore = FileInstallationIdentityStore(file, cipher)

    private fun assertSaveRejects(identity: InstallationIdentity) {
        assertThrows(IllegalArgumentException::class.java) { store().save(identity) }
    }

    private fun identityWithOptionalValues(): InstallationIdentity =
        InstallationIdentityFactory.newIdentity().copy(
            senderId = ByteArray(SENDER_ID_BYTES) { index -> (index + 1).toByte() },
            dedicatedServerKeys =
                DedicatedServerKeys(
                    x25519PublicKey = ByteArray(KEY_BYTES) { index -> (index + 2).toByte() },
                    ed25519PublicKey = ByteArray(KEY_BYTES) { index -> (index + 3).toByte() },
                ),
        )

    private fun validLines(
        signing: Ed25519KeyPair = Ed25519.generate(),
        agreement: X25519KeyPair = X25519.generate(),
        senderId: ByteArray? = null,
        serverX25519: ByteArray? = null,
        serverEd25519: ByteArray? = null,
    ): List<String> =
        buildList {
            add(formatLine(KEY_SIGNING_PRIVATE, signing.privateKey))
            add(formatLine(KEY_SIGNING_PUBLIC, signing.publicKey))
            add(formatLine(KEY_AGREEMENT_PRIVATE, agreement.privateKey))
            add(formatLine(KEY_AGREEMENT_PUBLIC, agreement.publicKey))
            senderId?.let { value -> add(formatLine(KEY_SENDER_ID, value)) }
            serverX25519?.let { value -> add(formatLine(KEY_SERVER_X25519, value)) }
            serverEd25519?.let { value -> add(formatLine(KEY_SERVER_ED25519, value)) }
        }

    private fun storeWithLines(lines: List<String>): FileInstallationIdentityStore = storeWithPlaintext(lines.joinToString(NEWLINE))

    private fun storeWithPlaintext(plaintext: String): FileInstallationIdentityStore = storeWithEnvelope(envelope(plaintext))

    private fun storeWithEnvelope(content: String): FileInstallationIdentityStore {
        val file = identityFile()
        file.writeText(content, Charsets.UTF_8)
        return store(file)
    }

    private fun envelope(plaintext: String): String = envelopeWithEncodedPayload(encodePayload(plaintext))

    private fun envelopeWithEncodedPayload(encoded: String): String = PAYLOAD_PREFIX + encoded + NEWLINE

    private fun encodePayload(plaintext: String): String =
        BASE64_ENCODER.encodeToString(cipher.encrypt(plaintext.toByteArray(Charsets.UTF_8)))

    private fun formatLine(
        key: String,
        value: ByteArray,
    ): String = "$key$KEY_SEPARATOR${BASE64_ENCODER.encodeToString(value)}"

    private fun List<String>.withEncodedValue(
        key: String,
        value: ByteArray,
    ): List<String> = withRawValue(key, BASE64_ENCODER.encodeToString(value))

    private fun List<String>.withRawValue(
        key: String,
        rawValue: String,
    ): List<String> =
        map { line ->
            if (line.startsWith("$key$KEY_SEPARATOR")) "$key$KEY_SEPARATOR$rawValue" else line
        }

    private fun assertIdentityEquals(
        expected: InstallationIdentity,
        actual: InstallationIdentity?,
    ) {
        assertNotNull(actual)
        val loaded = actual ?: return
        assertArrayEquals(expected.signingKeyPair.privateKey, loaded.signingKeyPair.privateKey)
        assertArrayEquals(expected.signingKeyPair.publicKey, loaded.signingKeyPair.publicKey)
        assertArrayEquals(expected.agreementKeyPair.privateKey, loaded.agreementKeyPair.privateKey)
        assertArrayEquals(expected.agreementKeyPair.publicKey, loaded.agreementKeyPair.publicKey)
        val expectedSenderId = expected.senderId
        if (expectedSenderId == null) {
            assertNull(loaded.senderId)
        } else {
            assertArrayEquals(expectedSenderId, loaded.senderId)
        }
        val expectedServerKeys = expected.dedicatedServerKeys
        if (expectedServerKeys == null) {
            assertNull(loaded.dedicatedServerKeys)
        } else {
            val loadedServerKeys = loaded.dedicatedServerKeys
            assertNotNull(loadedServerKeys)
            if (loadedServerKeys != null) {
                assertArrayEquals(expectedServerKeys.x25519PublicKey, loadedServerKeys.x25519PublicKey)
                assertArrayEquals(expectedServerKeys.ed25519PublicKey, loadedServerKeys.ed25519PublicKey)
            }
        }
    }

    private companion object {
        const val FILE_NAME = "installation-identity"
        const val NEWLINE = "\n"
        const val KEY_SEPARATOR = "="
        const val PAYLOAD_PREFIX = "payload="

        const val KEY_BYTES = 32
        const val SENDER_ID_BYTES = 16

        const val KEY_SIGNING_PRIVATE = "signingPrivate"
        const val KEY_SIGNING_PUBLIC = "signingPublic"
        const val KEY_AGREEMENT_PRIVATE = "agreementPrivate"
        const val KEY_AGREEMENT_PUBLIC = "agreementPublic"
        const val KEY_SENDER_ID = "senderId"
        const val KEY_SERVER_X25519 = "serverX25519"
        const val KEY_SERVER_ED25519 = "serverEd25519"

        const val UNKNOWN_KEY = "unknown"
        const val INVALID_BASE64 = "not-base64"
        const val BASE64_PADDING = '='

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
