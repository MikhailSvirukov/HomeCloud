package dev.homecloud.android.registration

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityFactory
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.ResultCodes
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/**
 * Asserts the installation-identity and durable-attempt behavior of the registration controller for
 * `FR-AND-028 / 2`, `FR-AND-029 / 1`, `FR-AND-053 / 1`, `FR-AND-054 / 1` and `FR-AND-055 / 1`.
 *
 * The identity store and pending store are in-memory doubles while the bootstrap and response
 * frames are built with real keys, so each scenario exercises the controller over the concrete
 * dedicated-server transport.
 */
class RegistrationControllerTest {
    private val serverX25519 = X25519.generate()
    private val serverEd25519 = Ed25519.generate()

    @Test
    fun `missing identity is generated and saved before a successful registration`() {
        val identityStore = ControllerIdentityStore(null)
        val pendingStore = ControllerPendingStore()
        var identityAtChannelOpen: InstallationIdentity? = null
        var identitySaveCountAtChannelOpen = -1
        var openedChannel: ControllerFrameChannel? = null
        val controller =
            registrationController(identityStore, pendingStore) { _, _ ->
                val stored = requireValue(identityStore.stored, "the identity was not created before the channel opened")
                val record = requireValue(pendingStore.record, "the pending record was not stored before the send")
                identityAtChannelOpen = stored
                identitySaveCountAtChannelOpen = identityStore.saveCount
                ControllerFrameChannel(successfulFrames(stored, record.requestId)).also { openedChannel = it }
            }

        val outcome = controller.register(HOST, PORT, LOGIN, PASSWORD)

        assertEquals(1, identitySaveCountAtChannelOpen)
        val created = requireValue(identityAtChannelOpen, "the identity was not created before the channel opened")
        assertEquals(KEY_BYTES, created.signingKeyPair.publicKey.size)
        assertEquals(KEY_BYTES, created.signingKeyPair.privateKey.size)
        assertEquals(KEY_BYTES, created.agreementKeyPair.publicKey.size)
        assertEquals(KEY_BYTES, created.agreementKeyPair.privateKey.size)
        val registered = outcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $outcome")
        assertArrayEquals(INSTALLATION_ID, registered.installationId)
        assertEquals(INSTALLATION_ID_BYTES, registered.installationId.size)
        assertEquals(1, requireValue(openedChannel, "the channel was not opened").writes.size)
        val saved = requireValue(identityStore.stored, "the registered identity was not saved")
        assertArrayEquals(created.signingKeyPair.privateKey, saved.signingKeyPair.privateKey)
        assertArrayEquals(created.signingKeyPair.publicKey, saved.signingKeyPair.publicKey)
        assertArrayEquals(created.agreementKeyPair.privateKey, saved.agreementKeyPair.privateKey)
        assertArrayEquals(created.agreementKeyPair.publicKey, saved.agreementKeyPair.publicKey)
        assertArrayEquals(INSTALLATION_ID, requireValue(saved.senderId, "the assigned id was not stored"))
    }

    @Test
    fun `second attempt reuses the stored identity without new keys or another identity save`() {
        val identityStore = ControllerIdentityStore(null)
        val pendingStore = ControllerPendingStore()
        val failed =
            registrationController(identityStore, pendingStore) { _, _ ->
                ControllerFrameChannel(emptyList(), readFailure = IOException(READ_FAILURE_MESSAGE))
            }.register(HOST, PORT, LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, failed)
        val created = requireValue(identityStore.stored, "the identity was not created")
        assertEquals(1, identityStore.saveCount)

        var identityAtChannelOpen: InstallationIdentity? = null
        var identitySaveCountAtChannelOpen = -1
        val controller =
            registrationController(identityStore, pendingStore) { _, _ ->
                val stored = requireValue(identityStore.stored, "the stored identity disappeared before the retry")
                val record = requireValue(pendingStore.record, "the pending record was cleared before the retry")
                identityAtChannelOpen = stored
                identitySaveCountAtChannelOpen = identityStore.saveCount
                ControllerFrameChannel(successfulFrames(stored, record.requestId))
            }

        val outcome = controller.register(HOST, PORT, LOGIN, PASSWORD)

        assertEquals(1, identitySaveCountAtChannelOpen)
        val reused = requireValue(identityAtChannelOpen, "the retry did not reuse the stored identity")
        assertArrayEquals(created.signingKeyPair.privateKey, reused.signingKeyPair.privateKey)
        assertArrayEquals(created.signingKeyPair.publicKey, reused.signingKeyPair.publicKey)
        assertArrayEquals(created.agreementKeyPair.privateKey, reused.agreementKeyPair.privateKey)
        assertArrayEquals(created.agreementKeyPair.publicKey, reused.agreementKeyPair.publicKey)
        val registered = outcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $outcome")
        assertArrayEquals(INSTALLATION_ID, registered.installationId)
    }

    @Test
    fun `identity save failure on first use reports StorageFailed before opening a channel`() {
        val identityStore = ControllerIdentityStore(null, saveFailure = true)
        val pendingStore = ControllerPendingStore()
        var channelFactoryCalls = 0

        val outcome =
            registrationController(identityStore, pendingStore) { _, _ ->
                channelFactoryCalls += 1
                ControllerFrameChannel(emptyList())
            }.register(HOST, PORT, LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.StorageFailed, outcome)
        assertEquals(0, channelFactoryCalls)
        assertEquals(0, pendingStore.saveCount)
        assertNull(pendingStore.record)
    }

    @Test
    fun `channel factory receives the host and port passed to register`() {
        val identityStore = ControllerIdentityStore(null)
        val pendingStore = ControllerPendingStore()
        var receivedHost: String? = null
        var receivedPort: Int? = null
        val controller =
            registrationController(identityStore, pendingStore) { host, port ->
                receivedHost = host
                receivedPort = port
                successChannel(identityStore, pendingStore)
            }

        val outcome = controller.register(HOST, PORT, LOGIN, PASSWORD)

        assertTrue(outcome is RegistrationOutcome.Registered)
        assertEquals(HOST, receivedHost)
        assertEquals(PORT, receivedPort)
    }

    @Test
    fun `channel read failure reports Pending and keeps the identity and pending record`() {
        val identity = InstallationIdentityFactory.newIdentity()
        val identityStore = ControllerIdentityStore(identity)
        val pendingStore = ControllerPendingStore()
        val channel = ControllerFrameChannel(emptyList(), readFailure = IOException(READ_FAILURE_MESSAGE))

        val outcome = registrationController(identityStore, pendingStore) { _, _ -> channel }.register(HOST, PORT, LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, outcome)
        val kept = requireValue(identityStore.stored, "the stored identity was removed")
        assertArrayEquals(identity.signingKeyPair.privateKey, kept.signingKeyPair.privateKey)
        assertArrayEquals(identity.signingKeyPair.publicKey, kept.signingKeyPair.publicKey)
        assertArrayEquals(identity.agreementKeyPair.privateKey, kept.agreementKeyPair.privateKey)
        assertArrayEquals(identity.agreementKeyPair.publicKey, kept.agreementKeyPair.publicKey)
        val record = requireValue(pendingStore.record, "the failed attempt cleared the pending record")
        assertEquals(LOGIN, record.login)
        assertEquals(PASSWORD, record.password)
        assertTrue(channel.closed)
    }

    @Test
    fun `successful registration clears the pending record and stores the response id`() {
        val identityStore = ControllerIdentityStore(InstallationIdentityFactory.newIdentity())
        val pendingStore = ControllerPendingStore()
        val controller =
            registrationController(identityStore, pendingStore) { _, _ ->
                successChannel(identityStore, pendingStore)
            }

        val outcome = controller.register(HOST, PORT, LOGIN, PASSWORD)

        val registered = outcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $outcome")
        assertArrayEquals(INSTALLATION_ID, registered.installationId)
        assertNull(pendingStore.record)
        assertEquals(1, pendingStore.clearCount)
        val saved = requireValue(identityStore.stored, "the registered identity was not saved")
        assertArrayEquals(registered.installationId, requireValue(saved.senderId, "the assigned id was not stored"))
    }

    @Test
    fun `pinned server keys are not saved again during registration`() {
        val identity =
            InstallationIdentityFactory.newIdentity().copy(
                dedicatedServerKeys = DedicatedServerKeys(serverX25519.publicKey, serverEd25519.publicKey),
            )
        val identityStore = ControllerIdentityStore(identity)
        val pendingStore = ControllerPendingStore()
        val controller =
            registrationController(identityStore, pendingStore) { _, _ ->
                successChannel(identityStore, pendingStore)
            }

        val outcome = controller.register(HOST, PORT, LOGIN, PASSWORD)

        assertTrue(outcome is RegistrationOutcome.Registered)
        assertEquals(1, identityStore.saveCount)
        val saved = requireValue(identityStore.stored, "the registered identity was not saved")
        assertArrayEquals(INSTALLATION_ID, requireValue(saved.senderId, "the assigned id was not stored"))
        assertArrayEquals(identity.signingKeyPair.publicKey, saved.signingKeyPair.publicKey)
        assertArrayEquals(identity.agreementKeyPair.publicKey, saved.agreementKeyPair.publicKey)
    }

    private fun registrationController(
        identityStore: ControllerIdentityStore,
        pendingStore: ControllerPendingStore,
        channelFactory: (String, Int) -> FrameChannel,
    ): RegistrationController =
        RegistrationController(
            identityStore = identityStore,
            pendingStore = pendingStore,
            channelFactory = channelFactory,
            maxFrameBytes = MAX_FRAME_BYTES,
            clock = { STARTED_AT_MILLIS },
        )

    private fun successChannel(
        identityStore: ControllerIdentityStore,
        pendingStore: ControllerPendingStore,
    ): ControllerFrameChannel {
        val identity = requireValue(identityStore.stored, "the installation identity is unavailable")
        val record = requireValue(pendingStore.record, "the pending record was not stored before the send")
        return ControllerFrameChannel(successfulFrames(identity, record.requestId))
    }

    private fun successfulFrames(
        identity: InstallationIdentity,
        requestId: ByteArray,
    ): List<ByteArray> =
        listOf(
            bootstrapFrame(),
            responseFrame(
                agreementPublicKey = identity.agreementKeyPair.publicKey,
                requestId = requestId,
            ),
        )

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
            sign = { bytes -> Ed25519.sign(serverEd25519.privateKey, bytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun responseFrame(
        agreementPublicKey: ByteArray,
        requestId: ByteArray,
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
            payload =
                FrameBodyCodec.encryptedPayload(
                    agreementPublicKey,
                    ResultEnvelopeCodec.encode(
                        ResultEnvelope(ResultCodes.SUCCESS, listOf(CborValue.Bytes(INSTALLATION_ID))),
                    ),
                ),
            sign = { bytes -> Ed25519.sign(serverEd25519.privateKey, bytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private companion object {
        const val MAX_FRAME_BYTES = 4_096
        const val KEY_BYTES = 32
        const val INSTALLATION_ID_BYTES = 16
        const val RESPONSE_FRAME_NUMBER = 2L
        const val HOST = "dedicated.homecloud.test"
        const val PORT = 7443
        const val LOGIN = "controller-login"
        const val PASSWORD = "controller-password"
        const val READ_FAILURE_MESSAGE = "scripted read failure"
        const val STARTED_AT_MILLIS = 1_700_000_000_000L

        val SERVER_SENDER_ID = ByteArray(16) { index -> (index + 1).toByte() }
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

/** In-memory identity store that records saves and can refuse to save (`FR-AND-028 / 2`, `FR-AND-029 / 1`). */
private class ControllerIdentityStore(
    var stored: InstallationIdentity?,
    var saveFailure: Boolean = false,
) : InstallationIdentityStore {
    var saveCount = 0

    override fun load(): InstallationIdentity? = stored

    override fun save(identity: InstallationIdentity) {
        if (saveFailure) throw IllegalStateException(SAVE_FAILURE_MESSAGE)
        saveCount += 1
        stored = identity
    }

    private companion object {
        const val SAVE_FAILURE_MESSAGE = "test identity store cannot save"
    }
}

/** In-memory pending store that records saves and clears (`FR-AND-053 / 1`). */
private class ControllerPendingStore(
    var record: PendingRegistration? = null,
) : PendingRegistrationStore {
    var saveCount = 0
    var clearCount = 0

    override fun load(): PendingRegistration? = record

    override fun save(record: PendingRegistration) {
        saveCount += 1
        this.record = record
    }

    override fun clear() {
        clearCount += 1
        record = null
    }
}

/** Frame channel that returns scripted frames, records writes and flags its own close (`INT-NET-081 / 1`). */
private class ControllerFrameChannel(
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
