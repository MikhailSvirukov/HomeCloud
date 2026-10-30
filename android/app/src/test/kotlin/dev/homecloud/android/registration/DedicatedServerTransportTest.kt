package dev.homecloud.android.registration

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ClientLogicalHeaderCodec
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.OperationType
import dev.homecloud.android.protocol.ResultCodes
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/**
 * Asserts the dedicated-server bootstrap handshake and the primary-registration frame of
 * `INT-NET-018`, `INT-NET-020`, `INT-NET-021`, `INT-NET-025`, `INT-NET-026`, `INT-NET-034`,
 * `INT-NET-050`, `INT-NET-058`, `INT-NET-059` and `INT-NET-081`.
 *
 * The channel is a scripted in-memory double, while the mutually verifying key material is real, so
 * the written registration frame is decoded from the bytes the transport produced.
 */
class DedicatedServerTransportTest {
    private val serverX25519 = X25519.generate()
    private val serverEd25519 = Ed25519.generate()
    private val identity =
        InstallationIdentity(
            signingKeyPair = Ed25519.generate(),
            agreementKeyPair = X25519.generate(),
            senderId = null,
            dedicatedServerKeys = null,
        )
    private val request =
        RegistrationRequest(
            requestId = REQUEST_ID,
            login = LOGIN,
            password = PASSWORD,
            x25519PublicKey = identity.agreementKeyPair.publicKey,
            ed25519PublicKey = identity.signingKeyPair.publicKey,
            arguments =
                RegistrationArguments.encode(
                    identity.agreementKeyPair.publicKey,
                    identity.signingKeyPair.publicKey,
                ),
        )

    @Test
    fun `successful registration returns the id and pins the server keys`() {
        val channel = scriptedChannel()
        val store = ScriptedInstallationIdentityStore(identity)

        val response = transport(store) { channel }.register(request)

        assertArrayEquals(INSTALLATION_ID, requireValue(response.installationId, "no installation id was returned"))
        assertEquals(1, store.saveCount)
        val pinned = requireValue(requireValue(store.stored, "the identity was not saved").dedicatedServerKeys, "no keys were pinned")
        assertArrayEquals(serverX25519.publicKey, pinned.x25519PublicKey)
        assertArrayEquals(serverEd25519.publicKey, pinned.ed25519PublicKey)
        val saved = requireValue(store.stored, "the identity was not saved")
        assertArrayEquals(identity.signingKeyPair.privateKey, saved.signingKeyPair.privateKey)
        assertArrayEquals(identity.signingKeyPair.publicKey, saved.signingKeyPair.publicKey)
        assertArrayEquals(identity.agreementKeyPair.privateKey, saved.agreementKeyPair.privateKey)
        assertArrayEquals(identity.agreementKeyPair.publicKey, saved.agreementKeyPair.publicKey)
        assertTrue(channel.closed)
    }

    @Test
    fun `registration frame carries the primary registration header and arguments`() {
        val channel = scriptedChannel()

        transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)

        val frame = channel.writes.single()
        val decoded =
            FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { signedBytes, signature ->
                Ed25519.verify(identity.signingKeyPair.publicKey, signedBytes, signature)
            }
        assertNull(decoded.header.senderId)
        assertArrayEquals(REQUEST_ID, decoded.header.requestId)
        assertEquals(0L, decoded.header.frameNumber)
        assertNull(decoded.header.uploadId)
        assertNull(decoded.header.responderLabel)
        assertArrayEquals(
            identity.signingKeyPair.publicKey,
            requireValue(decoded.header.bootstrapSigningPublicKey, "the bootstrap signing key is missing"),
        )
        val logical = ClientLogicalHeaderCodec.decode(FrameBodyCodec.decryptedPayload(serverX25519.privateKey, decoded.payload))
        assertEquals(OperationType.REGISTRATION, logical.operation)
        assertEquals(LOGIN, logical.login)
        assertEquals(PASSWORD, logical.password)
        assertEquals(2, logical.arguments.size)
        assertArrayEquals(identity.agreementKeyPair.publicKey, byteString(logical.arguments[0]))
        assertArrayEquals(identity.signingKeyPair.publicKey, byteString(logical.arguments[1]))
    }

    @Test
    fun `matching pinned keys are reused without saving the identity`() {
        val pinnedIdentity =
            identity.copy(
                dedicatedServerKeys = DedicatedServerKeys(serverX25519.publicKey, serverEd25519.publicKey),
            )
        val channel = scriptedChannel()
        val store = ScriptedInstallationIdentityStore(pinnedIdentity)

        val response = transport(store) { channel }.register(request)

        assertArrayEquals(INSTALLATION_ID, requireValue(response.installationId, "no installation id was returned"))
        assertEquals(0, store.saveCount)
        assertTrue(channel.closed)
    }

    @Test
    fun `different pinned keys fail without writing and close the channel`() {
        val otherX25519 = X25519.generate()
        val otherEd25519 = Ed25519.generate()
        val pinnedIdentity =
            identity.copy(
                dedicatedServerKeys = DedicatedServerKeys(otherX25519.publicKey, otherEd25519.publicKey),
            )
        val channel = ScriptedFrameChannel(listOf(bootstrapFrame()))
        val store = ScriptedInstallationIdentityStore(pinnedIdentity)

        assertThrows(RegistrationTransportException::class.java) {
            transport(store) { channel }.register(request)
        }

        assertTrue(channel.writes.isEmpty())
        assertEquals(0, store.saveCount)
        assertTrue(channel.closed)
    }

    @Test
    fun `a different pinned x25519 key fails without writing`() {
        val pinnedIdentity =
            identity.copy(
                dedicatedServerKeys = DedicatedServerKeys(X25519.generate().publicKey, serverEd25519.publicKey),
            )
        val channel = ScriptedFrameChannel(listOf(bootstrapFrame()))
        val store = ScriptedInstallationIdentityStore(pinnedIdentity)

        assertThrows(RegistrationTransportException::class.java) {
            transport(store) { channel }.register(request)
        }

        assertTrue(channel.writes.isEmpty())
        assertEquals(0, store.saveCount)
        assertTrue(channel.closed)
    }

    @Test
    fun `a different pinned ed25519 key fails without writing`() {
        val pinnedIdentity =
            identity.copy(
                dedicatedServerKeys = DedicatedServerKeys(serverX25519.publicKey, Ed25519.generate().publicKey),
            )
        val channel = ScriptedFrameChannel(listOf(bootstrapFrame()))
        val store = ScriptedInstallationIdentityStore(pinnedIdentity)

        assertThrows(RegistrationTransportException::class.java) {
            transport(store) { channel }.register(request)
        }

        assertTrue(channel.writes.isEmpty())
        assertEquals(0, store.saveCount)
        assertTrue(channel.closed)
    }

    @Test
    fun `response with a bad server signature fails`() {
        val channel =
            ScriptedFrameChannel(
                listOf(
                    bootstrapFrame(),
                    responseFrame(successEnvelope(CborValue.Bytes(INSTALLATION_ID)), signer = Ed25519.generate()),
                ),
            )

        assertThrows(RegistrationTransportException::class.java) {
            transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)
        }

        assertTrue(channel.closed)
    }

    @Test
    fun `response with a different request id is rejected and closes the channel`() {
        val channel =
            ScriptedFrameChannel(
                listOf(
                    bootstrapFrame(),
                    responseFrame(
                        successEnvelope(CborValue.Bytes(INSTALLATION_ID)),
                        requestId = DIFFERENT_REQUEST_ID,
                    ),
                ),
            )

        assertThrows(RegistrationTransportException::class.java) {
            transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)
        }

        assertEquals(1, channel.writes.size)
        assertTrue(channel.closed)
    }

    @Test
    fun `non-success response envelope has no installation id`() {
        val channel =
            ScriptedFrameChannel(
                listOf(
                    bootstrapFrame(),
                    responseFrame(ResultEnvelope(ResultCodes.REGISTRATION_CONFLICT, emptyList())),
                ),
            )

        val response = transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)

        assertNull(response.installationId)
    }

    @Test
    fun `success envelope with two arguments has no installation id`() {
        val channel =
            ScriptedFrameChannel(
                listOf(
                    bootstrapFrame(),
                    responseFrame(
                        successEnvelope(
                            CborValue.Bytes(INSTALLATION_ID),
                            CborValue.Bytes(INSTALLATION_ID),
                        ),
                    ),
                ),
            )

        val response = transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)

        assertNull(response.installationId)
    }

    @Test
    fun `success envelope with a fifteen byte id has no installation id`() {
        val channel =
            ScriptedFrameChannel(
                listOf(
                    bootstrapFrame(),
                    responseFrame(successEnvelope(CborValue.Bytes(ByteArray(INSTALLATION_ID_BYTES - 1)))),
                ),
            )

        val response = transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)

        assertNull(response.installationId)
    }

    @Test
    fun `missing identity fails without opening a channel`() {
        var channelFactoryCalls = 0

        assertThrows(RegistrationTransportException::class.java) {
            transport(ScriptedInstallationIdentityStore(null)) {
                channelFactoryCalls += 1
                ScriptedFrameChannel(emptyList())
            }.register(request)
        }

        assertEquals(0, channelFactoryCalls)
    }

    @Test
    fun `channel read failure is reported as a transport failure and closes the channel`() {
        val channel = ScriptedFrameChannel(emptyList(), readFailure = IOException(READ_FAILURE_MESSAGE))

        assertThrows(RegistrationTransportException::class.java) {
            transport(ScriptedInstallationIdentityStore(identity)) { channel }.register(request)
        }

        assertTrue(channel.closed)
    }

    private fun scriptedChannel(): ScriptedFrameChannel =
        ScriptedFrameChannel(
            listOf(
                bootstrapFrame(),
                responseFrame(successEnvelope(CborValue.Bytes(INSTALLATION_ID))),
            ),
        )

    @Test
    fun `success envelope with a sixteen byte non version four id has no installation id`() {
        val response =
            transport(ScriptedInstallationIdentityStore(identity)) {
                ScriptedFrameChannel(
                    listOf(
                        bootstrapFrame(),
                        responseFrame(successEnvelope(CborValue.Bytes(ByteArray(INSTALLATION_ID_BYTES)))),
                    ),
                )
            }.register(request)

        assertNull(response.installationId)
    }

    @Test
    fun `request with different installation keys is rejected before opening a channel`() {
        var channelFactoryCalls = 0
        val mismatched =
            request.copy(
                x25519PublicKey = X25519.generate().publicKey,
                ed25519PublicKey = Ed25519.generate().publicKey,
            )

        assertThrows(RegistrationTransportException::class.java) {
            transport(ScriptedInstallationIdentityStore(identity)) {
                channelFactoryCalls += 1
                scriptedChannel()
            }.register(mismatched)
        }
        assertEquals(0, channelFactoryCalls)
    }

    private fun transport(
        store: InstallationIdentityStore,
        channelFactory: () -> FrameChannel,
    ): DedicatedServerTransport = DedicatedServerTransport(store, channelFactory, MAX_FRAME_BYTES)

    private fun bootstrapFrame(): ByteArray =
        FrameBodyCodec.encode(
            header =
                ExternalFrameHeader(
                    senderId = SERVER_SENDER_ID,
                    requestId = BOOTSTRAP_REQUEST_ID,
                    frameNumber = 0,
                    uploadId = null,
                    responderLabel = null,
                    bootstrapSigningPublicKey = null,
                ),
            payload = FrameBodyCodec.bootstrapPayload(serverX25519.publicKey, serverEd25519.publicKey),
            sign = { signedBytes -> Ed25519.sign(serverEd25519.privateKey, signedBytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun responseFrame(
        envelope: ResultEnvelope,
        signer: Ed25519KeyPair = serverEd25519,
        requestId: ByteArray = REQUEST_ID,
    ): ByteArray =
        FrameBodyCodec.encode(
            header =
                ExternalFrameHeader(
                    senderId = SERVER_SENDER_ID,
                    requestId = requestId,
                    frameNumber = RESPONSE_FRAME_NUMBER,
                    uploadId = null,
                    responderLabel = null,
                    bootstrapSigningPublicKey = null,
                ),
            payload = FrameBodyCodec.encryptedPayload(identity.agreementKeyPair.publicKey, ResultEnvelopeCodec.encode(envelope)),
            sign = { signedBytes -> Ed25519.sign(signer.privateKey, signedBytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun successEnvelope(vararg arguments: CborValue): ResultEnvelope = ResultEnvelope(ResultCodes.SUCCESS, arguments.toList())

    private fun byteString(value: CborValue): ByteArray =
        when (value) {
            is CborValue.Bytes -> value.value
            else -> throw AssertionError("registration argument is not a CBOR byte string")
        }

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private companion object {
        const val MAX_FRAME_BYTES = 4_096
        const val INSTALLATION_ID_BYTES = 16
        const val RESPONSE_FRAME_NUMBER = 2L
        const val LOGIN = "test-login"
        const val PASSWORD = "test-password"
        const val READ_FAILURE_MESSAGE = "scripted read failure"

        val SERVER_SENDER_ID = ByteArray(16) { index -> (index + 1).toByte() }
        val REQUEST_ID = ByteArray(16) { index -> (index + 21).toByte() }
        val DIFFERENT_REQUEST_ID = ByteArray(16) { index -> (index + 61).toByte() }
        val BOOTSTRAP_REQUEST_ID = ByteArray(16) { index -> (index + 41).toByte() }
        val INSTALLATION_ID =
            byteArrayOf(
                0x12,
                0x34,
                0x56,
                0x78,
                0x9A.toByte(),
                0xBC.toByte(),
                0x4D,
                0xEF.toByte(),
                0x8A.toByte(),
                0xBC.toByte(),
                0xDE.toByte(),
                0xF0.toByte(),
                0x11,
                0x22,
                0x33,
                0x44,
            )
    }
}

/** In-memory identity store that records how often the first-contact pin was written (`INT-NET-025`). */
private class ScriptedInstallationIdentityStore(
    var stored: InstallationIdentity?,
) : InstallationIdentityStore {
    var saveCount = 0

    override fun load(): InstallationIdentity? = stored

    override fun save(identity: InstallationIdentity) {
        saveCount += 1
        stored = identity
    }
}

/** Frame channel that returns scripted frames, records writes and flags its own close (`INT-NET-081`). */
private class ScriptedFrameChannel(
    frames: List<ByteArray>,
    private val readFailure: IOException? = null,
) : FrameChannel {
    private val queuedFrames = frames.toMutableList()
    val writes = mutableListOf<ByteArray>()
    var closed = false
        private set

    override fun write(frame: ByteArray) {
        writes += frame.copyOf()
    }

    override fun read(): ByteArray {
        readFailure?.let { failure -> throw failure }
        if (queuedFrames.isEmpty()) throw AssertionError("no scripted frame remains")
        return queuedFrames.removeAt(0)
    }

    override fun close() {
        closed = true
    }
}
