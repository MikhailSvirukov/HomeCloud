package dev.homecloud.android.catalog

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ClientLogicalHeader
import dev.homecloud.android.protocol.ClientLogicalHeaderCodec
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.OperationType
import dev.homecloud.android.protocol.ResultCodes
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import dev.homecloud.android.protocol.TopologyNode
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.nio.ByteBuffer
import java.util.UUID

/**
 * Asserts the direct catalog-page exchange of `INT-NET-080`, `INT-NET-084`, `FR-CAT-012`,
 * `FR-CAT-013` and `FR-CAT-015`.
 *
 * The channel is a scripted in-memory double that builds its response from the request it receives,
 * while the node and installation keys are real, so every written request is verified, decrypted
 * and compared with the exact logical header the contract fixes.
 */
class CatalogClientTest {
    private val nodeX25519 = X25519.generate()
    private val nodeEd25519 = Ed25519.generate()
    private val identity =
        InstallationIdentity(
            signingKeyPair = Ed25519.generate(),
            agreementKeyPair = X25519.generate(),
            senderId = INSTALLATION_SENDER_ID,
            dedicatedServerKeys = null,
        )
    private val node =
        TopologyNode(
            senderId = NODE_SENDER_ID,
            label = NODE_LABEL,
            host = NODE_HOST,
            port = NODE_PORT,
            x25519PublicKey = nodeX25519.publicKey,
            ed25519PublicKey = nodeEd25519.publicKey,
        )

    @Test
    fun `last page returns the snapshot token both entries and a null next cursor`() {
        val channel =
            respondingChannel { request ->
                responseFrame(
                    request,
                    successPage(
                        snapshotToken = CborValue.Bytes(SNAPSHOT_TOKEN),
                        nextCursor = CborValue.Null,
                        entries =
                            listOf(
                                entryValue(),
                                entryValue(
                                    uploadId = CborValue.Bytes(SECOND_UPLOAD_ID),
                                    sourceName = CborValue.Text(SECOND_SOURCE_NAME),
                                    timestamp = CborValue.UInt(SECOND_TIMESTAMP_MILLIS),
                                    originalSize = CborValue.UInt(SECOND_ORIGINAL_SIZE),
                                    compressedSize = CborValue.UInt(SECOND_COMPRESSED_SIZE),
                                    sha256 = CborValue.Bytes(SECOND_SHA256),
                                ),
                            ),
                    ),
                )
            }

        val page = catalogClient(channelFactory = { _, _ -> channel }).fetchPage(node, LOGIN, PASSWORD, null, null)

        assertArrayEquals(SNAPSHOT_TOKEN, page.snapshotToken)
        assertNull(page.nextCursor)
        assertEquals(2, page.entries.size)
        val first = page.entries[0]
        assertArrayEquals(FIRST_UPLOAD_ID, first.uploadId)
        assertEquals(FIRST_SOURCE_NAME, first.sourceName)
        assertEquals(FIRST_TIMESTAMP_MILLIS, first.timestampMillis)
        assertEquals(FIRST_ORIGINAL_SIZE, first.originalSizeBytes)
        assertEquals(FIRST_COMPRESSED_SIZE, first.compressedSizeBytes)
        assertArrayEquals(FIRST_SHA256, first.sha256)
        val second = page.entries[1]
        assertArrayEquals(SECOND_UPLOAD_ID, second.uploadId)
        assertEquals(SECOND_SOURCE_NAME, second.sourceName)
        assertEquals(SECOND_TIMESTAMP_MILLIS, second.timestampMillis)
        assertEquals(SECOND_ORIGINAL_SIZE, second.originalSizeBytes)
        assertEquals(SECOND_COMPRESSED_SIZE, second.compressedSizeBytes)
        assertArrayEquals(SECOND_SHA256, second.sha256)
    }

    @Test
    fun `continuation request sends the supplied token and cursor unchanged and returns the next cursor`() {
        val channel =
            respondingChannel { request ->
                responseFrame(
                    request,
                    successPage(
                        snapshotToken = CborValue.Bytes(RESPONSE_SNAPSHOT_TOKEN),
                        nextCursor = CborValue.Bytes(NEXT_CURSOR_TOKEN),
                    ),
                )
            }

        val page =
            catalogClient(channelFactory = { _, _ -> channel })
                .fetchPage(node, LOGIN, PASSWORD, SNAPSHOT_TOKEN, CURSOR_TOKEN)

        assertArrayEquals(RESPONSE_SNAPSHOT_TOKEN, page.snapshotToken)
        assertArrayEquals(NEXT_CURSOR_TOKEN, requireValue(page.nextCursor, "no next cursor was returned"))
        val decoded = decodeWrittenRequest(channel.writes.single())
        assertArrayEquals(
            ClientLogicalHeaderCodec.encode(
                ClientLogicalHeader(
                    operation = OperationType.CATALOG,
                    login = LOGIN,
                    password = PASSWORD,
                    arguments = listOf(CborValue.Bytes(SNAPSHOT_TOKEN), CborValue.Bytes(CURSOR_TOKEN)),
                ),
            ),
            decoded.logicalBytes,
        )
    }

    @Test
    fun `first page request sends null for both arguments`() {
        val channel = respondingChannel { request -> responseFrame(request, successPage()) }

        catalogClient(channelFactory = { _, _ -> channel }).fetchPage(node, LOGIN, PASSWORD, null, null)

        val decoded = decodeWrittenRequest(channel.writes.single())
        assertArrayEquals(
            ClientLogicalHeaderCodec.encode(
                ClientLogicalHeader(
                    operation = OperationType.CATALOG,
                    login = LOGIN,
                    password = PASSWORD,
                    arguments = listOf(CborValue.Null, CborValue.Null),
                ),
            ),
            decoded.logicalBytes,
        )
        assertArrayEquals(
            INSTALLATION_SENDER_ID,
            requireValue(decoded.header.senderId, "the request sender id is missing"),
        )
        assertEquals(0L, decoded.header.frameNumber)
        assertEquals(REQUEST_ID_BYTES, decoded.header.requestId.size)
        assertNull(decoded.header.uploadId)
        assertNull(decoded.header.responderLabel)
        assertNull(decoded.header.bootstrapSigningPublicKey)
    }

    @Test
    fun `identity without a sender id is rejected before opening the channel`() {
        var channelFactoryCalls = 0

        assertThrows(CatalogException::class.java) {
            catalogClient(store = FixedIdentityStore(identity.copy(senderId = null))) { _, _ ->
                channelFactoryCalls += 1
                respondingChannel { request -> responseFrame(request, successPage()) }
            }.fetchPage(node, LOGIN, PASSWORD, null, null)
        }

        assertEquals(0, channelFactoryCalls)
    }

    @Test
    fun `snapshot token of the wrong length is rejected before opening the channel`() {
        var channelFactoryCalls = 0

        assertThrows(CatalogException::class.java) {
            catalogClient { _, _ ->
                channelFactoryCalls += 1
                respondingChannel { request -> responseFrame(request, successPage()) }
            }.fetchPage(node, LOGIN, PASSWORD, ByteArray(TOKEN_BYTES - 1), null)
        }

        assertEquals(0, channelFactoryCalls)
    }

    @Test
    fun `cursor of the wrong length is rejected before opening the channel`() {
        var channelFactoryCalls = 0

        assertThrows(CatalogException::class.java) {
            catalogClient { _, _ ->
                channelFactoryCalls += 1
                respondingChannel { request -> responseFrame(request, successPage()) }
            }.fetchPage(node, LOGIN, PASSWORD, null, ByteArray(TOKEN_BYTES - 1))
        }

        assertEquals(0, channelFactoryCalls)
    }

    @Test
    fun `non success result envelope names the code in hexadecimal`() {
        val channel =
            respondingChannel { request ->
                responseFrame(request, ResultEnvelope(ResultCodes.EXPIRED_SNAPSHOT, emptyList()))
            }

        val exception =
            assertThrows(CatalogException::class.java) {
                catalogClient(channelFactory = { _, _ -> channel })
                    .fetchPage(node, LOGIN, PASSWORD, null, null)
            }

        assertTrue(exception.message.orEmpty().contains("0x00040002"))
    }

    @Test
    fun `response signed with a different ed25519 key is rejected`() {
        val channel =
            respondingChannel { request ->
                responseFrame(request, successPage(), signer = Ed25519.generate())
            }

        assertThrows(CatalogException::class.java) {
            catalogClient(channelFactory = { _, _ -> channel })
                .fetchPage(node, LOGIN, PASSWORD, null, null)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `response with a different request id is rejected`() {
        val channel =
            respondingChannel { request ->
                responseFrame(request, successPage(), requestId = DIFFERENT_REQUEST_ID)
            }

        assertThrows(CatalogException::class.java) {
            catalogClient(channelFactory = { _, _ -> channel })
                .fetchPage(node, LOGIN, PASSWORD, null, null)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `page with two arguments is rejected`() {
        assertPageRejected(
            ResultEnvelope(
                ResultCodes.SUCCESS,
                listOf(CborValue.Bytes(SNAPSHOT_TOKEN), CborValue.Null),
            ),
        )
    }

    @Test
    fun `page with a fifteen byte snapshot token is rejected`() {
        assertPageRejected(successPage(snapshotToken = CborValue.Bytes(ByteArray(TOKEN_BYTES - 1))))
    }

    @Test
    fun `page with a fifteen byte next cursor is rejected`() {
        assertPageRejected(successPage(nextCursor = CborValue.Bytes(ByteArray(TOKEN_BYTES - 1))))
    }

    @Test
    fun `entry with five elements is rejected`() {
        assertPageRejected(successPage(entries = listOf(entryWithFiveElements())))
    }

    @Test
    fun `entry with a fifteen byte upload id is rejected`() {
        assertPageRejected(
            successPage(
                entries = listOf(entryValue(uploadId = CborValue.Bytes(ByteArray(UPLOAD_ID_BYTES - 1)))),
            ),
        )
    }

    @Test
    fun `entry with a negative original size is rejected`() {
        assertPageRejected(successPage(entries = listOf(entryValue(originalSize = CborValue.NInt(-1)))))
    }

    @Test
    fun `entry with a thirty one byte sha256 is rejected`() {
        assertPageRejected(
            successPage(
                entries = listOf(entryValue(sha256 = CborValue.Bytes(ByteArray(SHA256_BYTES - 1)))),
            ),
        )
    }

    @Test
    fun `entry with text where the upload id is expected is rejected`() {
        assertPageRejected(
            successPage(entries = listOf(entryValue(uploadId = CborValue.Text(UPLOAD_ID_TEXT)))),
        )
    }

    @Test
    fun `channel read failure is wrapped and closes the channel`() {
        val channel = FailingReadFrameChannel(IOException(READ_FAILURE_MESSAGE))

        val exception =
            assertThrows(CatalogException::class.java) {
                catalogClient(channelFactory = { _, _ -> channel })
                    .fetchPage(node, LOGIN, PASSWORD, null, null)
            }

        assertTrue(exception.cause is IOException)
        assertTrue(channel.closed)
    }

    private fun assertPageRejected(envelope: ResultEnvelope) {
        val channel = respondingChannel { request -> responseFrame(request, envelope) }

        assertThrows(CatalogException::class.java) {
            catalogClient(channelFactory = { _, _ -> channel })
                .fetchPage(node, LOGIN, PASSWORD, null, null)
        }
    }

    private fun catalogClient(channelFactory: (String, Int) -> FrameChannel): CatalogClient =
        catalogClient(FixedIdentityStore(identity), channelFactory)

    private fun catalogClient(
        store: InstallationIdentityStore,
        channelFactory: (String, Int) -> FrameChannel,
    ): CatalogClient = CatalogClient(store, channelFactory, MAX_FRAME_BYTES)

    private fun respondingChannel(respond: (request: ByteArray) -> ByteArray): RespondingFrameChannel = RespondingFrameChannel(respond)

    private fun decodeWrittenRequest(request: ByteArray): DecodedRequest {
        val decoded =
            FrameBodyCodec.decode(request, MAX_FRAME_BYTES) { signedBytes, signature ->
                Ed25519.verify(identity.signingKeyPair.publicKey, signedBytes, signature)
            }
        return DecodedRequest(
            header = decoded.header,
            logicalBytes = FrameBodyCodec.decryptedPayload(nodeX25519.privateKey, decoded.payload),
        )
    }

    private fun responseFrame(
        request: ByteArray,
        envelope: ResultEnvelope,
        signer: Ed25519KeyPair = nodeEd25519,
        requestId: ByteArray = requestIdOf(request),
    ): ByteArray =
        FrameBodyCodec.encode(
            header =
                ExternalFrameHeader(
                    senderId = NODE_SENDER_ID,
                    requestId = requestId,
                    frameNumber = RESPONSE_FRAME_NUMBER,
                    uploadId = null,
                    responderLabel = NODE_LABEL,
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

    private fun successPage(
        snapshotToken: CborValue = CborValue.Bytes(SNAPSHOT_TOKEN),
        nextCursor: CborValue = CborValue.Null,
        entries: List<CborValue> = emptyList(),
    ): ResultEnvelope =
        ResultEnvelope(
            ResultCodes.SUCCESS,
            listOf(CborValue.Array(listOf(snapshotToken, nextCursor, CborValue.Array(entries)))),
        )

    private fun entryValue(
        uploadId: CborValue = CborValue.Bytes(FIRST_UPLOAD_ID),
        sourceName: CborValue = CborValue.Text(FIRST_SOURCE_NAME),
        timestamp: CborValue = CborValue.UInt(FIRST_TIMESTAMP_MILLIS),
        originalSize: CborValue = CborValue.UInt(FIRST_ORIGINAL_SIZE),
        compressedSize: CborValue = CborValue.UInt(FIRST_COMPRESSED_SIZE),
        sha256: CborValue = CborValue.Bytes(FIRST_SHA256),
    ): CborValue = CborValue.Array(listOf(uploadId, sourceName, timestamp, originalSize, compressedSize, sha256))

    private fun entryWithFiveElements(): CborValue =
        CborValue.Array(
            listOf(
                CborValue.Bytes(FIRST_UPLOAD_ID),
                CborValue.Text(FIRST_SOURCE_NAME),
                CborValue.UInt(FIRST_TIMESTAMP_MILLIS),
                CborValue.UInt(FIRST_ORIGINAL_SIZE),
                CborValue.UInt(FIRST_COMPRESSED_SIZE),
            ),
        )

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private data class DecodedRequest(
        val header: ExternalFrameHeader,
        val logicalBytes: ByteArray,
    )

    private companion object {
        const val MAX_FRAME_BYTES = 4_096
        const val TOKEN_BYTES = 16
        const val REQUEST_ID_BYTES = 16
        const val UPLOAD_ID_BYTES = 16
        const val SHA256_BYTES = 32
        const val RESPONSE_FRAME_NUMBER = 0L

        const val LOGIN = "test-login"
        const val PASSWORD = "test-password"
        const val NODE_LABEL = "node-alpha"
        const val NODE_HOST = "127.0.0.1"
        const val NODE_PORT = 8443
        const val FIRST_SOURCE_NAME = "photos/first.jpg"
        const val SECOND_SOURCE_NAME = "photos/second.jpg"
        const val FIRST_TIMESTAMP_MILLIS = 1_700_000_000_123L
        const val SECOND_TIMESTAMP_MILLIS = 1_700_000_001_456L
        const val FIRST_ORIGINAL_SIZE = 8_388_608L
        const val FIRST_COMPRESSED_SIZE = 4_194_304L
        const val SECOND_ORIGINAL_SIZE = 2_097_152L
        const val SECOND_COMPRESSED_SIZE = 1_048_576L
        const val READ_FAILURE_MESSAGE = "scripted read failure"
        const val UPLOAD_ID_TEXT = "not an upload id"

        val INSTALLATION_SENDER_ID = uuidBytes("6f9619ff-8b86-4d01-b42d-00c04fc964ff")
        val NODE_SENDER_ID = uuidBytes("0f8fad5b-d9cb-469f-a165-70867728950e")
        val FIRST_UPLOAD_ID = uuidBytes("7c9e6679-7425-40de-944b-e07fc1f90ae7")
        val SECOND_UPLOAD_ID = uuidBytes("16fd2706-8baf-433b-82eb-8c7fada847da")
        val DIFFERENT_REQUEST_ID = uuidBytes("a3b2c1d0-e4f5-4678-9abc-def012345678")
        val SNAPSHOT_TOKEN = ByteArray(TOKEN_BYTES) { index -> (index + 1).toByte() }
        val CURSOR_TOKEN = ByteArray(TOKEN_BYTES) { index -> (index + 31).toByte() }
        val NEXT_CURSOR_TOKEN = ByteArray(TOKEN_BYTES) { index -> (index + 61).toByte() }
        val RESPONSE_SNAPSHOT_TOKEN = ByteArray(TOKEN_BYTES) { index -> (index + 91).toByte() }
        val FIRST_SHA256 = ByteArray(SHA256_BYTES) { index -> (index + 1).toByte() }
        val SECOND_SHA256 = ByteArray(SHA256_BYTES) { index -> (index + 101).toByte() }
    }
}

/** In-memory identity store that always produces the same identity. */
private class FixedIdentityStore(
    private val stored: InstallationIdentity,
) : InstallationIdentityStore {
    override fun load(): InstallationIdentity = stored

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
