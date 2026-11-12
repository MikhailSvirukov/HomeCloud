package dev.homecloud.android.upload

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.crypto.X25519KeyPair
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.payload.CompressedPayload
import dev.homecloud.android.protocol.CborDecoder
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ClientLogicalHeaderCodec
import dev.homecloud.android.protocol.DecodedFrameBody
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.OperationType
import dev.homecloud.android.protocol.ResultCodes
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import dev.homecloud.android.protocol.TopologyNode
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/**
 * Asserts the direct upload exchange of `FR-UP-003`, `FR-UP-004`, `FR-UP-005`, `FR-UP-006`,
 * `FR-UP-016`, `FR-UP-019` and `FR-UP-021`.
 *
 * The channel is a scripted in-memory double that builds its response from the frames it receives,
 * while the node and installation keys are real, so every written frame is verified, decrypted and
 * compared with the values derived from the request.
 */
class UploadClientTest {
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
    fun `one frame upload carries the logical header sender id upload id and payload`() {
        val payload = payload(PAYLOAD_BLOCK_SIZE)
        val request = request(payload = payload)
        val channel = respondingChannel { writes -> successResponse(writes.first(), request.uploadId) }

        uploadClient(channelFactory = { _, _ -> channel }).upload(request)

        val decoded = decodeFrame(channel.writes.single())
        assertArrayEquals(
            INSTALLATION_SENDER_ID,
            requireValue(decoded.header.senderId, "the request sender id is missing"),
        )
        assertArrayEquals(
            request.uploadId,
            requireValue(decoded.header.uploadId, "the request upload id is missing"),
        )
        assertEquals(0L, decoded.header.frameNumber)
        assertEquals(REQUEST_ID_BYTES, decoded.header.requestId.size)
        assertNull(decoded.header.responderLabel)
        assertNull(decoded.header.bootstrapSigningPublicKey)
        val plaintext = FrameBodyCodec.decryptedPayload(nodeX25519.privateKey, decoded.payload)
        val logicalBytes = CborDecoder.decodePrefix(plaintext, 0).consumedBytes
        val logical = ClientLogicalHeaderCodec.decode(plaintext.copyOfRange(0, logicalBytes))
        assertEquals(OperationType.UPLOAD, logical.operation)
        assertEquals(LOGIN, logical.login)
        assertEquals(PASSWORD, logical.password)
        assertMetadataArguments(logical.arguments)
        assertArrayEquals(
            payloadBytes(payload),
            plaintext.copyOfRange(logicalBytes, plaintext.size),
        )
    }

    @Test
    fun `successful response returns the confirmed upload id`() {
        val request = request()
        val channel = respondingChannel { writes -> successResponse(writes.first(), request.uploadId) }

        val confirmed = uploadClient(channelFactory = { _, _ -> channel }).upload(request)

        assertArrayEquals(request.uploadId, confirmed)
    }

    @Test
    fun `multi frame upload numbers frames without gaps and carries payload only after the first header`() {
        val payload = payload(180, 160, 140, 120)
        val request = request(payload = payload)
        val channel = respondingChannel { writes -> successResponse(writes.first(), request.uploadId) }

        uploadClient(maxFrameBytes = PLAN_MAX_FRAME_BYTES, channelFactory = { _, _ -> channel }).upload(request)

        val writes = channel.writes
        assertTrue(writes.size > 1)
        val decodedFrames = writes.map { frame -> decodeFrame(frame) }
        val expectedPayload = payloadBytes(payload)
        val firstPlaintext =
            FrameBodyCodec.decryptedPayload(nodeX25519.privateKey, decodedFrames.first().payload)
        val firstLogicalBytes = CborDecoder.decodePrefix(firstPlaintext, 0).consumedBytes
        var offset = firstPlaintext.size - firstLogicalBytes
        for (index in writes.indices) {
            val declaredLength = declaredBodyLength(writes[index])
            assertEquals(index.toLong(), decodedFrames[index].header.frameNumber)
            assertEquals(writes[index].size.toLong() - LENGTH_PREFIX_BYTES, declaredLength)
            assertTrue(declaredLength <= PLAN_MAX_FRAME_BYTES.toLong())
            if (index > 0) {
                val plaintext =
                    FrameBodyCodec.decryptedPayload(nodeX25519.privateKey, decodedFrames[index].payload)
                assertArrayEquals(expectedPayload.copyOfRange(offset, offset + plaintext.size), plaintext)
                offset += plaintext.size
            }
        }
        assertEquals(expectedPayload.size, offset)
        val reconstructed =
            writes.drop(1).fold(firstPlaintext.copyOfRange(firstLogicalBytes, firstPlaintext.size)) { bytes, frame ->
                bytes + FrameBodyCodec.decryptedPayload(nodeX25519.privateKey, decodeFrame(frame).payload)
            }
        assertArrayEquals(expectedPayload, reconstructed)
    }

    @Test
    fun `success response with a different upload id is rejected`() {
        val request = request()
        val channel = respondingChannel { writes -> successResponse(writes.first(), DIFFERENT_UPLOAD_ID) }

        assertThrows(UploadException::class.java) {
            uploadClient(channelFactory = { _, _ -> channel }).upload(request)
        }
        assertTrue(channel.closed)
    }

    @Test
    fun `success response with no arguments is rejected`() {
        assertRejectedResponse(ResultEnvelope(ResultCodes.SUCCESS, emptyList()))
    }

    @Test
    fun `success response with two arguments is rejected`() {
        assertRejectedResponse(
            ResultEnvelope(
                ResultCodes.SUCCESS,
                listOf(CborValue.Bytes(UPLOAD_ID), CborValue.Bytes(DIFFERENT_UPLOAD_ID)),
            ),
        )
    }

    @Test
    fun `non zero result code is rejected with the code in hexadecimal`() {
        val channel =
            respondingChannel { writes ->
                responseFrame(
                    request = writes.first(),
                    envelope = ResultEnvelope(ResultCodes.UPLOAD_ID_CONFLICT, emptyList()),
                )
            }

        val exception =
            assertThrows(UploadException::class.java) {
                uploadClient(channelFactory = { _, _ -> channel }).upload(request())
            }

        assertTrue(exception.message.orEmpty().contains("0x00050004"))
        assertTrue(channel.closed)
    }

    @Test
    fun `response with another request id is rejected`() {
        assertRejectedResponse(
            envelope = successEnvelope(UPLOAD_ID),
            requestId = { DIFFERENT_REQUEST_ID },
        )
    }

    @Test
    fun `response signed with another ed25519 key is rejected`() {
        assertRejectedResponse(successEnvelope(UPLOAD_ID), signer = Ed25519.generate())
    }

    @Test
    fun `channel read failure is wrapped and closes the channel`() {
        val channel = FailingReadFrameChannel(IOException(READ_FAILURE_MESSAGE))

        val exception =
            assertThrows(UploadException::class.java) {
                uploadClient(channelFactory = { _, _ -> channel }).upload(request())
            }

        assertTrue(exception.cause is IOException)
        assertTrue(channel.closed)
    }

    @Test
    fun `retry of the same upload id starts again at frame zero with a different request id`() {
        val request = request()
        val firstChannel = respondingChannel { writes -> successResponse(writes.first(), request.uploadId) }
        val secondChannel = respondingChannel { writes -> successResponse(writes.first(), request.uploadId) }
        val channels = ArrayDeque(listOf(firstChannel, secondChannel))
        val client = uploadClient(channelFactory = { _, _ -> channels.removeFirst() })

        client.upload(request)
        client.upload(request)

        val first = decodeFrame(firstChannel.writes.first())
        val second = decodeFrame(secondChannel.writes.first())
        assertEquals(0L, first.header.frameNumber)
        assertEquals(0L, second.header.frameNumber)
        assertArrayEquals(
            request.uploadId,
            requireValue(first.header.uploadId, "the retry upload id is missing"),
        )
        assertArrayEquals(
            request.uploadId,
            requireValue(second.header.uploadId, "the retry upload id is missing"),
        )
        assertFalse(first.header.requestId.contentEquals(second.header.requestId))
    }

    @Test
    fun `absent identity is rejected before the channel factory is invoked`() {
        assertIdentityRejected(stored = null)
    }

    @Test
    fun `identity without a sender id is rejected before the channel factory is invoked`() {
        assertIdentityRejected(identity.copy(senderId = null))
    }

    @Test
    fun `identity without key pairs is rejected before the channel factory is invoked`() {
        assertIdentityRejected(
            identity.copy(
                signingKeyPair = Ed25519KeyPair(ByteArray(0), ByteArray(0)),
                agreementKeyPair = X25519KeyPair(ByteArray(0), ByteArray(0)),
            ),
        )
    }

    @Test
    fun `upload id of the wrong length is rejected before the channel is opened`() {
        assertRequestRejected(request(uploadId = ByteArray(UPLOAD_ID_BYTES - 1)))
    }

    @Test
    fun `metadata compressed size different from the payload is rejected before the channel is opened`() {
        assertRequestRejected(
            request(
                payload = payload(PAYLOAD_BLOCK_SIZE),
                metadata = metadata(compressedSizeBytes = PAYLOAD_BLOCK_SIZE + 1L),
            ),
        )
    }

    @Test
    fun `original size above the configured metadata limit is rejected before the channel is opened`() {
        assertRequestRejected(
            request = request(metadata = metadata(originalSizeBytes = SMALL_ORIGINAL_LIMIT_BYTES + 1)),
            metadataLimits = UploadMetadataLimits(maxOriginalSizeBytes = SMALL_ORIGINAL_LIMIT_BYTES),
        )
    }

    private fun assertMetadataArguments(arguments: List<CborValue>) {
        assertEquals(METADATA_ARGUMENT_COUNT, arguments.size)
        assertEquals(CborValue.Text(SOURCE_NAME), arguments[0])
        assertEquals(CborValue.UInt(TIMESTAMP_MILLIS), arguments[1])
        assertEquals(CborValue.UInt(ORIGINAL_SIZE_BYTES), arguments[2])
        assertEquals(CborValue.UInt(DEFAULT_COMPRESSED_SIZE_BYTES), arguments[3])
        assertArrayEquals(SHA256, (arguments[4] as CborValue.Bytes).value)
    }

    private fun assertRejectedResponse(
        envelope: ResultEnvelope,
        signer: Ed25519KeyPair = nodeEd25519,
        requestId: (ByteArray) -> ByteArray = { frame -> requestIdOf(frame) },
    ) {
        val channel =
            respondingChannel { writes ->
                responseFrame(
                    request = writes.first(),
                    envelope = envelope,
                    signer = signer,
                    requestId = requestId(writes.first()),
                )
            }

        assertThrows(UploadException::class.java) {
            uploadClient(channelFactory = { _, _ -> channel }).upload(request())
        }
        assertTrue(channel.closed)
    }

    private fun assertIdentityRejected(stored: InstallationIdentity?) {
        var channelFactoryCalls = 0
        val store: InstallationIdentityStore =
            if (stored == null) AbsentIdentityStore() else FixedIdentityStore(stored)

        assertThrows(UploadException::class.java) {
            uploadClient(store = store) { _, _ ->
                channelFactoryCalls += 1
                unusedChannel()
            }.upload(request())
        }

        assertEquals(0, channelFactoryCalls)
    }

    private fun assertRequestRejected(
        request: UploadRequest,
        metadataLimits: UploadMetadataLimits = LIMITS,
    ) {
        var channelFactoryCalls = 0

        assertThrows(UploadException::class.java) {
            uploadClient(metadataLimits = metadataLimits) { _, _ ->
                channelFactoryCalls += 1
                unusedChannel()
            }.upload(request)
        }

        assertEquals(0, channelFactoryCalls)
    }

    private fun uploadClient(
        store: InstallationIdentityStore = FixedIdentityStore(identity),
        metadataLimits: UploadMetadataLimits = LIMITS,
        maxFrameBytes: Int = MAX_FRAME_BYTES,
        channelFactory: (host: String, port: Int) -> FrameChannel,
    ): UploadClient = UploadClient(store, channelFactory, metadataLimits, maxFrameBytes)

    private fun respondingChannel(respond: (writes: List<ByteArray>) -> ByteArray): RespondingFrameChannel = RespondingFrameChannel(respond)

    private fun decodeFrame(frame: ByteArray): DecodedFrameBody =
        FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { bytes, signature ->
            Ed25519.verify(identity.signingKeyPair.publicKey, bytes, signature)
        }

    private fun requestIdOf(frame: ByteArray): ByteArray = decodeFrame(frame).header.requestId

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
            sign = { bytes -> Ed25519.sign(signer.privateKey, bytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun successResponse(
        request: ByteArray,
        uploadId: ByteArray,
    ): ByteArray = responseFrame(request, successEnvelope(uploadId))

    private fun successEnvelope(uploadId: ByteArray): ResultEnvelope =
        ResultEnvelope(ResultCodes.SUCCESS, listOf(CborValue.Bytes(uploadId)))

    private fun request(
        uploadId: ByteArray = UPLOAD_ID,
        payload: CompressedPayload = payload(PAYLOAD_BLOCK_SIZE),
        metadata: UploadMetadata = metadata(compressedSizeBytes = payload.totalBytes),
    ): UploadRequest = UploadRequest(uploadId, node, LOGIN, PASSWORD, metadata, payload)

    private fun metadata(
        originalName: String = SOURCE_NAME,
        timestampMillis: Long = TIMESTAMP_MILLIS,
        originalSizeBytes: Long = ORIGINAL_SIZE_BYTES,
        compressedSizeBytes: Long = DEFAULT_COMPRESSED_SIZE_BYTES,
        sha256: ByteArray = SHA256,
    ): UploadMetadata =
        UploadMetadata(
            originalName = originalName,
            timestampMillis = timestampMillis,
            originalSizeBytes = originalSizeBytes,
            compressedSizeBytes = compressedSizeBytes,
            sha256 = sha256,
        )

    private fun payload(vararg blockSizes: Int): CompressedPayload =
        CompressedPayload(
            blockSizes.mapIndexed { blockIndex, size ->
                ByteArray(size) { offset -> (blockIndex * BLOCK_FILL_STEP + offset).toByte() }
            },
        )

    private fun payloadBytes(payload: CompressedPayload): ByteArray = payload.blocks.fold(ByteArray(0)) { bytes, block -> bytes + block }

    private fun declaredBodyLength(frame: ByteArray): Long {
        var length = 0L
        for (index in 0 until LENGTH_PREFIX_BYTES) {
            length = (length shl BITS_PER_BYTE) or (frame[index].toLong() and BYTE_MASK)
        }
        return length
    }

    private fun unusedChannel(): FrameChannel = RespondingFrameChannel { throw AssertionError(CHANNEL_MUST_NOT_OPEN_MESSAGE) }

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private companion object {
        const val MAX_FRAME_BYTES = 4_096
        const val PLAN_MAX_FRAME_BYTES = 512
        const val UPLOAD_ID_BYTES = 16
        const val REQUEST_ID_BYTES = 16
        const val SHA256_BYTES = 32
        const val METADATA_ARGUMENT_COUNT = 5
        const val LENGTH_PREFIX_BYTES = 4
        const val BITS_PER_BYTE = 8
        const val BYTE_MASK = 0xFFL
        const val BLOCK_FILL_STEP = 31
        const val PAYLOAD_BLOCK_SIZE = 64
        const val DEFAULT_COMPRESSED_SIZE_BYTES = 64L
        const val RESPONSE_FRAME_NUMBER = 0L
        const val MAX_ORIGINAL_SIZE_BYTES = 1_000_000L
        const val SMALL_ORIGINAL_LIMIT_BYTES = 512L

        const val LOGIN = "test-login"
        const val PASSWORD = "test-password"
        const val NODE_LABEL = "node-alpha"
        const val NODE_HOST = "127.0.0.1"
        const val NODE_PORT = 8443
        const val SOURCE_NAME = "photos/clip.jpg"
        const val TIMESTAMP_MILLIS = 1_700_000_000_123L
        const val ORIGINAL_SIZE_BYTES = 12_345L
        const val READ_FAILURE_MESSAGE = "scripted read failure"
        const val CHANNEL_MUST_NOT_OPEN_MESSAGE = "the channel must not be opened"

        val LIMITS = UploadMetadataLimits(maxOriginalSizeBytes = MAX_ORIGINAL_SIZE_BYTES)
        val INSTALLATION_SENDER_ID = bytes(UPLOAD_ID_BYTES, 1)
        val NODE_SENDER_ID = bytes(UPLOAD_ID_BYTES, 21)
        val UPLOAD_ID = bytes(UPLOAD_ID_BYTES, 41)
        val DIFFERENT_UPLOAD_ID = bytes(UPLOAD_ID_BYTES, 61)
        val DIFFERENT_REQUEST_ID = bytes(REQUEST_ID_BYTES, 81)
        val SHA256 = bytes(SHA256_BYTES, 101)
    }
}

private fun bytes(
    size: Int,
    start: Int,
): ByteArray = ByteArray(size) { index -> (start + index).toByte() }

/** In-memory identity store that always produces the same identity. */
private class FixedIdentityStore(
    private val stored: InstallationIdentity,
) : InstallationIdentityStore {
    override fun load(): InstallationIdentity = stored

    override fun save(identity: InstallationIdentity) = Unit
}

/** Identity store without a stored identity. */
private class AbsentIdentityStore : InstallationIdentityStore {
    override fun load(): InstallationIdentity? = null

    override fun save(identity: InstallationIdentity) = Unit
}

/** Frame channel that records every write and builds its response at read time. */
private class RespondingFrameChannel(
    private val respond: (writes: List<ByteArray>) -> ByteArray,
) : FrameChannel {
    val writes = mutableListOf<ByteArray>()
    var closed = false
        private set

    override fun write(frame: ByteArray) {
        writes += frame.copyOf()
    }

    override fun read(): ByteArray = respond(writes)

    override fun close() {
        closed = true
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
