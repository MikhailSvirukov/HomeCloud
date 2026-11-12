package dev.homecloud.android.topology

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.ResultCodes
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import dev.homecloud.android.protocol.TopologyCodec
import dev.homecloud.android.protocol.TopologyNode
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.nio.ByteBuffer
import java.util.UUID

/**
 * Asserts the dedicated-server topology exchange of `INT-NET-025`, `INT-NET-034`, `INT-NET-060`,
 * `INT-NET-070` and `CFG-CL-031`.
 *
 * The channel is a scripted in-memory double that builds its response from the request it receives,
 * while the installation and server keys are real, so every written request is verified, decrypted
 * and compared with the exact logical header the contract fixes.
 */
class TopologyClientTest {
    private val serverX25519 = X25519.generate()
    private val serverEd25519 = Ed25519.generate()
    private val identity =
        InstallationIdentity(
            signingKeyPair = Ed25519.generate(),
            agreementKeyPair = X25519.generate(),
            senderId = INSTALLATION_SENDER_ID,
            dedicatedServerKeys = DedicatedServerKeys(serverX25519.publicKey, serverEd25519.publicKey),
        )
    private val firstNode =
        TopologyNode(
            senderId = FIRST_NODE_SENDER_ID,
            label = FIRST_NODE_LABEL,
            host = FIRST_NODE_HOST,
            port = FIRST_NODE_PORT,
            x25519PublicKey = X25519.generate().publicKey,
            ed25519PublicKey = Ed25519.generate().publicKey,
        )
    private val secondNode =
        TopologyNode(
            senderId = SECOND_NODE_SENDER_ID,
            label = SECOND_NODE_LABEL,
            host = SECOND_NODE_HOST,
            port = SECOND_NODE_PORT,
            x25519PublicKey = X25519.generate().publicKey,
            ed25519PublicKey = Ed25519.generate().publicKey,
        )

    @Test
    fun `successful answer returns the nodes in answer order and closes the channel`() {
        val channel = respondingChannel { request -> responseFrame(request, successAnswer(listOf(firstNode, secondNode))) }
        var requestedHost: String? = null
        var requestedPort: Int? = null

        val nodes =
            topologyClient(channelFactory = { host, port ->
                requestedHost = host
                requestedPort = port
                channel
            }).fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(SERVER_HOST, requestedHost)
        assertEquals(SERVER_PORT, requestedPort)
        assertEquals(2, nodes.size)
        assertNodeEquals(firstNode, nodes[0])
        assertNodeEquals(secondNode, nodes[1])
        assertTrue(channel.closed)
    }

    @Test
    fun `written request decrypts to the topology logical header and carries the expected header fields`() {
        val channel = respondingChannel { request -> responseFrame(request, successAnswer(listOf(firstNode))) }

        topologyClient(channelFactory = { _, _ -> channel }).fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        val decoded = decodeWrittenRequest(channel.writes.single())
        assertArrayEquals(TopologyCodec.encodeRequest(LOGIN, PASSWORD), decoded.logicalBytes)
        assertArrayEquals(
            INSTALLATION_SENDER_ID,
            requireValue(decoded.header.senderId, "the request sender id is missing"),
        )
        assertEquals(REQUEST_ID_BYTES, decoded.header.requestId.size)
        assertEquals(0L, decoded.header.frameNumber)
        assertNull(decoded.header.uploadId)
        assertNull(decoded.header.responderLabel)
        assertNull(decoded.header.bootstrapSigningPublicKey)
    }

    @Test
    fun `non zero result code is rejected with the code in hexadecimal`() {
        val channel =
            respondingChannel { request ->
                responseFrame(
                    request,
                    ResultEnvelope(ResultCodes.AUTHENTICATION_REJECTED, emptyList()),
                )
            }

        val exception =
            assertThrows(TopologyException::class.java) {
                topologyClient(channelFactory = { _, _ -> channel })
                    .fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
            }

        assertTrue(exception.message.orEmpty().contains("0x00020001"))
        assertTrue(channel.closed)
    }

    @Test
    fun `successful answer with no argument is rejected`() {
        val channel =
            respondingChannel { request ->
                responseFrame(request, ResultEnvelope(ResultCodes.SUCCESS, emptyList()))
            }

        assertThrows(TopologyException::class.java) {
            topologyClient(channelFactory = { _, _ -> channel })
                .fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `response with a different request id is rejected`() {
        val channel =
            respondingChannel { request ->
                responseFrame(
                    request,
                    successAnswer(listOf(firstNode)),
                    requestId = DIFFERENT_REQUEST_ID,
                )
            }

        assertThrows(TopologyException::class.java) {
            topologyClient(channelFactory = { _, _ -> channel })
                .fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `response signed with a different ed25519 key is rejected`() {
        val channel =
            respondingChannel { request ->
                responseFrame(
                    request,
                    successAnswer(listOf(firstNode)),
                    signer = Ed25519.generate(),
                )
            }

        assertThrows(TopologyException::class.java) {
            topologyClient(channelFactory = { _, _ -> channel })
                .fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `channel read failure is wrapped and closes the channel`() {
        val channel = FailingReadFrameChannel(IOException(READ_FAILURE_MESSAGE))

        val exception =
            assertThrows(TopologyException::class.java) {
                topologyClient(channelFactory = { _, _ -> channel })
                    .fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
            }

        assertTrue(exception.cause is IOException)
        assertTrue(channel.closed)
    }

    @Test
    fun `missing identity is rejected before opening the channel`() {
        assertRejectedBeforeOpening(AbsentIdentityStore())
    }

    @Test
    fun `missing sender id is rejected before opening the channel`() {
        assertRejectedBeforeOpening(FixedIdentityStore(identity.copy(senderId = null)))
    }

    @Test
    fun `installation key pair that is not thirty two bytes is rejected before opening the channel`() {
        val shortKeyPair = Ed25519KeyPair(ByteArray(KEY_BYTES - 1), identity.signingKeyPair.publicKey)

        assertRejectedBeforeOpening(
            FixedIdentityStore(identity.copy(signingKeyPair = shortKeyPair)),
        )
    }

    @Test
    fun `missing pinned key set is rejected before opening the channel`() {
        assertRejectedBeforeOpening(FixedIdentityStore(identity.copy(dedicatedServerKeys = null)))
    }

    @Test
    fun `pinned key that is not thirty two bytes is rejected before opening the channel`() {
        val shortServerKeys = DedicatedServerKeys(ByteArray(KEY_BYTES - 1), serverEd25519.publicKey)

        assertRejectedBeforeOpening(
            FixedIdentityStore(identity.copy(dedicatedServerKeys = shortServerKeys)),
        )
    }

    @Test
    fun `node label above the configured utf8 byte limit is rejected`() {
        val label = MULTIBYTE_LABEL
        val node = firstNode.copy(label = label)
        val channel = respondingChannel { request -> responseFrame(request, successAnswer(listOf(node))) }

        val exception =
            assertThrows(TopologyException::class.java) {
                topologyClient(
                    maxLabelBytes = label.toByteArray(Charsets.UTF_8).size - 1,
                    channelFactory = { _, _ -> channel },
                ).fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
            }

        assertFalse(exception.message.orEmpty().contains(label))
        assertTrue(channel.closed)
    }

    @Test
    fun `node label exactly at the configured utf8 byte limit is accepted`() {
        val label = MULTIBYTE_LABEL
        val node = firstNode.copy(label = label)
        val channel = respondingChannel { request -> responseFrame(request, successAnswer(listOf(node))) }

        val nodes =
            topologyClient(
                maxLabelBytes = label.toByteArray(Charsets.UTF_8).size,
                channelFactory = { _, _ -> channel },
            ).fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(label, nodes.single().label)
    }

    private fun assertRejectedBeforeOpening(store: InstallationIdentityStore) {
        var channelFactoryCalls = 0

        assertThrows(TopologyException::class.java) {
            topologyClient(store = store) { _, _ ->
                channelFactoryCalls += 1
                unusedChannel()
            }.fetch(SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)
        }

        assertEquals(0, channelFactoryCalls)
    }

    private fun topologyClient(
        store: InstallationIdentityStore = FixedIdentityStore(identity),
        maxLabelBytes: Int = MAX_LABEL_BYTES,
        channelFactory: (String, Int) -> FrameChannel,
    ): TopologyClient = TopologyClient(store, channelFactory, MAX_FRAME_BYTES, maxLabelBytes)

    private fun respondingChannel(respond: (request: ByteArray) -> ByteArray): RespondingFrameChannel = RespondingFrameChannel(respond)

    private fun decodeWrittenRequest(request: ByteArray): DecodedRequest {
        val decoded =
            FrameBodyCodec.decode(request, MAX_FRAME_BYTES) { signedBytes, signature ->
                Ed25519.verify(identity.signingKeyPair.publicKey, signedBytes, signature)
            }
        return DecodedRequest(
            header = decoded.header,
            logicalBytes = FrameBodyCodec.decryptedPayload(serverX25519.privateKey, decoded.payload),
        )
    }

    private fun responseFrame(
        request: ByteArray,
        envelope: ResultEnvelope,
        signer: Ed25519KeyPair = serverEd25519,
        requestId: ByteArray = requestIdOf(request),
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
                    identity.agreementKeyPair.publicKey,
                    ResultEnvelopeCodec.encode(envelope),
                ),
            sign = { signedBytes -> Ed25519.sign(signer.privateKey, signedBytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun requestIdOf(request: ByteArray): ByteArray =
        FrameBodyCodec
            .decode(request, MAX_FRAME_BYTES) { signedBytes, signature ->
                Ed25519.verify(identity.signingKeyPair.publicKey, signedBytes, signature)
            }.header
            .requestId

    private fun successAnswer(nodes: List<TopologyNode>): ResultEnvelope =
        ResultEnvelope(
            ResultCodes.SUCCESS,
            listOf(CborValue.Array(nodes.map { node -> nodeValue(node) })),
        )

    private fun nodeValue(node: TopologyNode): CborValue =
        CborValue.Array(
            listOf(
                CborValue.Bytes(node.senderId),
                CborValue.Text(node.label),
                CborValue.Text(node.host),
                CborValue.UInt(node.port.toLong()),
                CborValue.Bytes(node.x25519PublicKey),
                CborValue.Bytes(node.ed25519PublicKey),
            ),
        )

    private fun assertNodeEquals(
        expected: TopologyNode,
        actual: TopologyNode,
    ) {
        assertArrayEquals(expected.senderId, actual.senderId)
        assertEquals(expected.label, actual.label)
        assertEquals(expected.host, actual.host)
        assertEquals(expected.port, actual.port)
        assertArrayEquals(expected.x25519PublicKey, actual.x25519PublicKey)
        assertArrayEquals(expected.ed25519PublicKey, actual.ed25519PublicKey)
    }

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private fun unusedChannel(): FrameChannel = RespondingFrameChannel { throw AssertionError("the channel must not be opened") }

    private data class DecodedRequest(
        val header: ExternalFrameHeader,
        val logicalBytes: ByteArray,
    )

    private companion object {
        const val MAX_FRAME_BYTES = 4_096
        const val MAX_LABEL_BYTES = 128
        const val KEY_BYTES = 32
        const val REQUEST_ID_BYTES = 16
        const val RESPONSE_FRAME_NUMBER = 0L

        const val LOGIN = "test-login"
        const val PASSWORD = "test-password"
        const val SERVER_HOST = "127.0.0.1"
        const val SERVER_PORT = 8443
        const val FIRST_NODE_LABEL = "node-alpha"
        const val SECOND_NODE_LABEL = "node-beta"
        const val FIRST_NODE_HOST = "127.0.0.2"
        const val SECOND_NODE_HOST = "2001:db8::1"
        const val FIRST_NODE_PORT = 8444
        const val SECOND_NODE_PORT = 8445
        const val READ_FAILURE_MESSAGE = "scripted read failure"
        const val MULTIBYTE_LABEL = "ééé"

        val INSTALLATION_SENDER_ID = uuidBytes("6f9619ff-8b86-4d01-b42d-00c04fc964ff")
        val SERVER_SENDER_ID = uuidBytes("0f8fad5b-d9cb-469f-a165-70867728950e")
        val FIRST_NODE_SENDER_ID = uuidBytes("0f8fad5b-d9cb-469f-a165-70867728950f")
        val SECOND_NODE_SENDER_ID = uuidBytes("0f8fad5b-d9cb-469f-a165-708677289510")
        val DIFFERENT_REQUEST_ID = uuidBytes("a3b2c1d0-e4f5-4678-9abc-def012345678")
    }
}

/** In-memory identity store that always produces the same identity. */
private class FixedIdentityStore(
    private val stored: InstallationIdentity,
) : InstallationIdentityStore {
    override fun load(): InstallationIdentity = stored

    override fun save(identity: InstallationIdentity) = Unit
}

/** In-memory identity store that holds no installation identity. */
private class AbsentIdentityStore : InstallationIdentityStore {
    override fun load(): InstallationIdentity? = null

    override fun save(identity: InstallationIdentity) = Unit
}

/** Frame channel that builds its response from the frame just written and records every write. */
private class RespondingFrameChannel(
    private val respond: (request: ByteArray) -> ByteArray,
) : FrameChannel {
    val writes = mutableListOf<ByteArray>()
    var closed = false
        private set
    private var response: ByteArray? = null

    override fun write(frame: ByteArray) {
        writes += frame.copyOf()
        response = respond(frame)
    }

    override fun read(): ByteArray = response ?: throw AssertionError(NO_RESPONSE_MESSAGE)

    override fun close() {
        closed = true
    }

    private companion object {
        const val NO_RESPONSE_MESSAGE = "no response was scripted"
    }
}

/** Frame channel that records its close flag while every read fails with the scripted failure. */
private class FailingReadFrameChannel(
    private val failure: IOException,
) : FrameChannel {
    var closed = false
        private set

    override fun write(frame: ByteArray) = Unit

    override fun read(): ByteArray = throw failure

    override fun close() {
        closed = true
    }
}

private fun uuidBytes(value: String): ByteArray {
    val uuid = UUID.fromString(value)
    return ByteBuffer
        .allocate(UUID_BYTES)
        .putLong(uuid.mostSignificantBits)
        .putLong(uuid.leastSignificantBits)
        .array()
}

private const val UUID_BYTES = 16
