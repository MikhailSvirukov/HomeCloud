package dev.homecloud.android.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolMessagesTest {
    @Test
    fun externalHeaderEncodesSevenPositionsForOrdinaryMessage() {
        val header =
            ExternalFrameHeader(
                senderId = SENDER_ID,
                requestId = REQUEST_ID,
                frameNumber = FRAME_NUMBER,
                uploadId = UPLOAD_ID,
                responderLabel = LABEL,
                bootstrapSigningPublicKey = null,
            )

        val elements = arrayItems(ExternalFrameHeaderCodec.encode(header))

        assertEquals(EXTERNAL_HEADER_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(PROTOCOL_VERSION), elements[0])
        assertBytes(SENDER_ID, elements[1])
        assertBytes(REQUEST_ID, elements[2])
        assertEquals(CborValue.UInt(FRAME_NUMBER), elements[3])
        assertBytes(UPLOAD_ID, elements[4])
        assertEquals(CborValue.Text(LABEL), elements[5])
        assertEquals(CborValue.Null, elements[6])
    }

    @Test
    fun primaryRegistrationHeaderEncodesNullSenderBootstrapKeyAndNullUploadId() {
        val header =
            ExternalFrameHeader(
                senderId = null,
                requestId = REQUEST_ID,
                frameNumber = FIRST_FRAME_NUMBER,
                uploadId = null,
                responderLabel = null,
                bootstrapSigningPublicKey = BOOTSTRAP_SIGNING_KEY,
            )

        val elements = arrayItems(ExternalFrameHeaderCodec.encode(header))

        assertEquals(EXTERNAL_HEADER_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(PROTOCOL_VERSION), elements[0])
        assertEquals(CborValue.Null, elements[1])
        assertBytes(REQUEST_ID, elements[2])
        assertEquals(CborValue.UInt(FIRST_FRAME_NUMBER), elements[3])
        assertEquals(CborValue.Null, elements[4])
        assertEquals(CborValue.Null, elements[5])
        assertBytes(BOOTSTRAP_SIGNING_KEY, elements[6])
    }

    @Test
    fun encodeRejectsNullSenderAndNullBootstrapSigningKey() {
        val header =
            ExternalFrameHeader(
                senderId = null,
                requestId = REQUEST_ID,
                frameNumber = FRAME_NUMBER,
                uploadId = null,
                responderLabel = null,
                bootstrapSigningPublicKey = null,
            )

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.encode(header) }
    }

    @Test
    fun encodeRejectsSenderAndBootstrapSigningKeyTogether() {
        val header =
            ExternalFrameHeader(
                senderId = SENDER_ID,
                requestId = REQUEST_ID,
                frameNumber = FRAME_NUMBER,
                uploadId = null,
                responderLabel = null,
                bootstrapSigningPublicKey = BOOTSTRAP_SIGNING_KEY,
            )

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.encode(header) }
    }

    @Test
    fun decodeReadsOrdinaryHeaderSenderAndNullBootstrapSigningKey() {
        val header =
            ExternalFrameHeader(
                senderId = SENDER_ID,
                requestId = REQUEST_ID,
                frameNumber = FRAME_NUMBER,
                uploadId = UPLOAD_ID,
                responderLabel = LABEL,
                bootstrapSigningPublicKey = null,
            )

        val decoded = ExternalFrameHeaderCodec.decode(ExternalFrameHeaderCodec.encode(header))

        assertArrayEquals(SENDER_ID, decoded.senderId)
        assertNull(decoded.bootstrapSigningPublicKey)
    }

    @Test
    fun decodeRejectsNullSenderAndNullBootstrapSigningKey() {
        val encoded =
            externalHeaderArray(
                senderId = CborValue.Null,
                bootstrapSigningPublicKey = CborValue.Null,
            )

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsSenderAndBootstrapSigningKeyTogether() {
        val encoded =
            externalHeaderArray(
                senderId = CborValue.Bytes(SENDER_ID),
                bootstrapSigningPublicKey = CborValue.Bytes(BOOTSTRAP_SIGNING_KEY),
            )

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsFifteenByteSenderId() {
        val encoded = externalHeaderArray(senderId = CborValue.Bytes(SHORT_BYTE_STRING))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsFifteenByteUploadId() {
        val encoded = externalHeaderArray(uploadId = CborValue.Bytes(SHORT_BYTE_STRING))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsNullRequestId() {
        val encoded = externalHeaderArray(requestId = CborValue.Null)

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsProtocolVersionOtherThanOne() {
        val encoded = externalHeaderArray(version = OTHER_PROTOCOL_VERSION)

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsWrongExternalHeaderLength() {
        val encoded = CborEncoder.encode(CborValue.Array(List(SHORT_HEADER_ELEMENTS) { CborValue.Null }))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsFifteenByteRequestId() {
        val encoded = externalHeaderArray(requestId = CborValue.Bytes(SHORT_BYTE_STRING))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsFrameNumberAboveUint32() {
        val encoded = externalHeaderArray(frameNumber = UINT32_OVERFLOW)

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsThirtyOneByteBootstrapSigningKey() {
        val encoded = externalHeaderArray(bootstrapSigningPublicKey = CborValue.Bytes(SHORT_BOOTSTRAP_SIGNING_KEY))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsThirtyOneByteBootstrapSigningKeyWithNullSenderId() {
        val encoded =
            externalHeaderArray(
                senderId = CborValue.Null,
                bootstrapSigningPublicKey = CborValue.Bytes(SHORT_BOOTSTRAP_SIGNING_KEY),
            )

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsNonTextResponderLabel() {
        val encoded = externalHeaderArray(responderLabel = CborValue.UInt(1))

        assertThrows(ProtocolEncodingException::class.java) { ExternalFrameHeaderCodec.decode(encoded) }
    }

    @Test
    fun topologyLogicalHeaderEncodesOperationCredentialsAndEmptyArguments() {
        val encoded =
            ClientLogicalHeaderCodec.encode(
                ClientLogicalHeader(
                    operation = OperationType.TOPOLOGY,
                    login = LOGIN,
                    password = PASSWORD,
                    arguments = emptyList(),
                ),
            )

        val elements = arrayItems(encoded)

        assertEquals(CLIENT_LOGICAL_HEADER_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(TOPOLOGY_CODE), elements[0])
        assertEquals(CborValue.Text(LOGIN), elements[1])
        assertEquals(CborValue.Text(PASSWORD), elements[2])
        assertEquals(CborValue.Array(emptyList()), elements[3])

        val decoded = ClientLogicalHeaderCodec.decode(encoded)

        assertEquals(OperationType.TOPOLOGY, decoded.operation)
        assertEquals(LOGIN, decoded.login)
        assertEquals(PASSWORD, decoded.password)
        assertTrue(decoded.arguments.isEmpty())
    }

    @Test
    fun decodeRejectsUnknownOperationCodes() {
        listOf(UNKNOWN_OPERATION_ZERO, UNKNOWN_OPERATION_FIVE).forEach { code ->
            val encoded =
                CborEncoder.encode(
                    CborValue.Array(
                        listOf(
                            CborValue.UInt(code),
                            CborValue.Text(LOGIN),
                            CborValue.Text(PASSWORD),
                            CborValue.Array(emptyList()),
                        ),
                    ),
                )

            assertThrows(ProtocolEncodingException::class.java) { ClientLogicalHeaderCodec.decode(encoded) }
        }
    }

    @Test
    fun decodeRejectsThreeElementLogicalHeader() {
        val encoded =
            CborEncoder.encode(
                CborValue.Array(
                    listOf(
                        CborValue.UInt(TOPOLOGY_CODE),
                        CborValue.Text(LOGIN),
                        CborValue.Text(PASSWORD),
                    ),
                ),
            )

        assertThrows(ProtocolEncodingException::class.java) { ClientLogicalHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsNonTextLogin() {
        val encoded = logicalHeaderArray(login = CborValue.UInt(NON_TEXT_VALUE))

        assertThrows(ProtocolEncodingException::class.java) { ClientLogicalHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsNonTextPassword() {
        val encoded = logicalHeaderArray(password = CborValue.UInt(NON_TEXT_VALUE))

        assertThrows(ProtocolEncodingException::class.java) { ClientLogicalHeaderCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsNonArrayOperationArguments() {
        val encoded = logicalHeaderArray(arguments = CborValue.Text(LOGIN))

        assertThrows(ProtocolEncodingException::class.java) { ClientLogicalHeaderCodec.decode(encoded) }
    }

    @Test
    fun successResultEnvelopeEncodesZeroCodeAndEmptyArguments() {
        val elements = arrayItems(ResultEnvelopeCodec.encode(ResultEnvelope(ResultCodes.SUCCESS, emptyList())))

        assertEquals(RESULT_ENVELOPE_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(ResultCodes.SUCCESS), elements[0])
        assertEquals(CborValue.Array(emptyList()), elements[1])
    }

    @Test
    fun errorResultEnvelopeEncodesCodeAndEmptyArguments() {
        val elements =
            arrayItems(ResultEnvelopeCodec.encode(ResultEnvelope(ResultCodes.AUTHENTICATION_REJECTED, emptyList())))

        assertEquals(RESULT_ENVELOPE_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(ResultCodes.AUTHENTICATION_REJECTED), elements[0])
        assertEquals(CborValue.Array(emptyList()), elements[1])
    }

    @Test
    fun isSuccessIsTrueOnlyForZeroResultCode() {
        assertTrue(ResultEnvelope(ResultCodes.SUCCESS, emptyList()).isSuccess)
        assertFalse(ResultEnvelope(ResultCodes.INVALID_ARGUMENTS, emptyList()).isSuccess)
        assertFalse(ResultEnvelope(NON_CONTRACT_RESULT_CODE, emptyList()).isSuccess)
    }

    @Test
    fun decodeRejectsNegativeResultCode() {
        val encoded = CborEncoder.encode(CborValue.Array(listOf(CborValue.NInt(-1), CborValue.Array(emptyList()))))

        assertThrows(ProtocolEncodingException::class.java) { ResultEnvelopeCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsOneElementResultEnvelope() {
        val encoded = CborEncoder.encode(CborValue.Array(listOf(CborValue.UInt(ResultCodes.SUCCESS))))

        assertThrows(ProtocolEncodingException::class.java) { ResultEnvelopeCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsThreeElementResultEnvelope() {
        val encoded =
            CborEncoder.encode(
                CborValue.Array(
                    listOf(
                        CborValue.UInt(ResultCodes.SUCCESS),
                        CborValue.Array(emptyList()),
                        CborValue.Null,
                    ),
                ),
            )

        assertThrows(ProtocolEncodingException::class.java) { ResultEnvelopeCodec.decode(encoded) }
    }

    @Test
    fun decodeRejectsResultCodeAboveUint32() {
        val encoded =
            CborEncoder.encode(
                CborValue.Array(listOf(CborValue.UInt(UINT32_OVERFLOW), CborValue.Array(emptyList()))),
            )

        assertThrows(ProtocolEncodingException::class.java) { ResultEnvelopeCodec.decode(encoded) }
    }

    @Test
    fun encodeRequestEncodesTopologyOperationWithCredentialsAndEmptyArguments() {
        val elements = arrayItems(TopologyCodec.encodeRequest(LOGIN, PASSWORD))

        assertEquals(CLIENT_LOGICAL_HEADER_ELEMENTS, elements.size)
        assertEquals(CborValue.UInt(TOPOLOGY_CODE), elements[0])
        assertEquals(CborValue.Text(LOGIN), elements[1])
        assertEquals(CborValue.Text(PASSWORD), elements[2])
        assertEquals(CborValue.Array(emptyList()), elements[3])
    }

    @Test
    fun decodeAnswerReadsSingleNode() {
        val nodes = TopologyCodec.decodeAnswer(topologyAnswer(node()))

        assertEquals(1, nodes.size)
        val node = nodes[0]
        assertArrayEquals(SENDER_ID, node.senderId)
        assertEquals(LABEL, node.label)
        assertEquals(HOST, node.host)
        assertEquals(PORT, node.port)
        assertArrayEquals(X25519_PUBLIC_KEY, node.x25519PublicKey)
        assertArrayEquals(ED25519_PUBLIC_KEY, node.ed25519PublicKey)
    }

    @Test
    fun decodeAnswerReadsTwoNodesInOrder() {
        val nodes =
            TopologyCodec.decodeAnswer(
                topologyAnswer(
                    node(),
                    node(senderId = OTHER_SENDER_ID, label = OTHER_LABEL, port = OTHER_PORT),
                ),
            )

        assertEquals(TWO_NODES, nodes.size)
        assertArrayEquals(SENDER_ID, nodes[0].senderId)
        assertArrayEquals(OTHER_SENDER_ID, nodes[1].senderId)
        assertEquals(listOf(LABEL, OTHER_LABEL), nodes.map { it.label })
        assertEquals(listOf(PORT, OTHER_PORT), nodes.map { it.port })
    }

    @Test
    fun decodeAnswerRejectsNonSuccessEnvelope() {
        val envelope = ResultEnvelope(ResultCodes.QUORUM_UNAVAILABLE, listOf(CborValue.Array(emptyList())))

        val exception = assertThrows(ProtocolEncodingException::class.java) { TopologyCodec.decodeAnswer(envelope) }

        assertTrue(exception.message.orEmpty().contains(QUORUM_UNAVAILABLE_HEX))
    }

    @Test
    fun decodeAnswerRejectsMissingNodesArgument() {
        val envelope = ResultEnvelope(ResultCodes.SUCCESS, emptyList())

        assertThrows(ProtocolEncodingException::class.java) { TopologyCodec.decodeAnswer(envelope) }
    }

    @Test
    fun decodeAnswerRejectsNodeWithFiveElements() {
        val incompleteNode =
            CborValue.Array(
                listOf(
                    CborValue.Bytes(SENDER_ID),
                    CborValue.Text(LABEL),
                    CborValue.Text(HOST),
                    CborValue.UInt(PORT.toLong()),
                    CborValue.Bytes(X25519_PUBLIC_KEY),
                ),
            )

        assertThrows(ProtocolEncodingException::class.java) {
            TopologyCodec.decodeAnswer(topologyAnswer(incompleteNode))
        }
    }

    @Test
    fun decodeAnswerRejectsFifteenByteSenderId() {
        assertThrows(ProtocolEncodingException::class.java) {
            TopologyCodec.decodeAnswer(topologyAnswer(node(senderId = SHORT_BYTE_STRING)))
        }
    }

    @Test
    fun decodeAnswerRejectsPortOutsideRange() {
        listOf(PORT_ZERO, PORT_ABOVE_RANGE).forEach { port ->
            assertThrows(ProtocolEncodingException::class.java) {
                TopologyCodec.decodeAnswer(topologyAnswer(node(port = port)))
            }
        }
    }

    @Test
    fun decodeAnswerRejectsThirtyOneByteX25519PublicKey() {
        assertThrows(ProtocolEncodingException::class.java) {
            TopologyCodec.decodeAnswer(topologyAnswer(node(x25519PublicKey = SHORT_PUBLIC_KEY)))
        }
    }

    @Test
    fun decodeAnswerRejectsThirtyOneByteEd25519PublicKey() {
        assertThrows(ProtocolEncodingException::class.java) {
            TopologyCodec.decodeAnswer(topologyAnswer(node(ed25519PublicKey = SHORT_PUBLIC_KEY)))
        }
    }

    @Test
    fun resultCodesMatchPublishedContractValues() {
        assertEquals(ResultCodes.SUCCESS, CONTRACT_SUCCESS)
        assertEquals(ResultCodes.UNKNOWN_OPERATION, CONTRACT_UNKNOWN_OPERATION)
        assertEquals(ResultCodes.INVALID_ARGUMENTS, CONTRACT_INVALID_ARGUMENTS)
        assertEquals(ResultCodes.INVALID_REQUEST_STATE, CONTRACT_INVALID_REQUEST_STATE)
        assertEquals(ResultCodes.REPEATED_REQUEST_ID, CONTRACT_REPEATED_REQUEST_ID)
        assertEquals(ResultCodes.AUTHENTICATION_REJECTED, CONTRACT_AUTHENTICATION_REJECTED)
        assertEquals(ResultCodes.REGISTRATION_CONFLICT, CONTRACT_REGISTRATION_CONFLICT)
        assertEquals(ResultCodes.REGISTRATION_REJECTED, CONTRACT_REGISTRATION_REJECTED)
        assertEquals(ResultCodes.INVALID_CURSOR, CONTRACT_INVALID_CURSOR)
        assertEquals(ResultCodes.EXPIRED_SNAPSHOT, CONTRACT_EXPIRED_SNAPSHOT)
        assertEquals(ResultCodes.FILE_SIZE_LIMIT, CONTRACT_FILE_SIZE_LIMIT)
        assertEquals(ResultCodes.SIZE_METADATA_OR_HASH_MISMATCH, CONTRACT_SIZE_METADATA_OR_HASH_MISMATCH)
        assertEquals(ResultCodes.INVALID_FRAME_SEQUENCE, CONTRACT_INVALID_FRAME_SEQUENCE)
        assertEquals(ResultCodes.UPLOAD_ID_CONFLICT, CONTRACT_UPLOAD_ID_CONFLICT)
        assertEquals(ResultCodes.QUORUM_UNAVAILABLE, CONTRACT_QUORUM_UNAVAILABLE)
        assertEquals(ResultCodes.BLOB_QUORUM_NOT_REACHED, CONTRACT_BLOB_QUORUM_NOT_REACHED)
        assertEquals(ResultCodes.TEMPORARY_UNAVAILABLE, CONTRACT_TEMPORARY_UNAVAILABLE)
        assertEquals(ResultCodes.STORAGE_CAPACITY_EXHAUSTED, CONTRACT_STORAGE_CAPACITY_EXHAUSTED)
        assertEquals(ResultCodes.INTERNAL_ERROR, CONTRACT_INTERNAL_ERROR)
    }

    private fun externalHeaderArray(
        version: Long = PROTOCOL_VERSION,
        senderId: CborValue = CborValue.Bytes(SENDER_ID),
        requestId: CborValue = CborValue.Bytes(REQUEST_ID),
        frameNumber: Long = FRAME_NUMBER,
        uploadId: CborValue = CborValue.Null,
        responderLabel: CborValue = CborValue.Null,
        bootstrapSigningPublicKey: CborValue = CborValue.Null,
    ): ByteArray =
        CborEncoder.encode(
            CborValue.Array(
                listOf(
                    CborValue.UInt(version),
                    senderId,
                    requestId,
                    CborValue.UInt(frameNumber),
                    uploadId,
                    responderLabel,
                    bootstrapSigningPublicKey,
                ),
            ),
        )

    private fun logicalHeaderArray(
        operation: Long = TOPOLOGY_CODE,
        login: CborValue = CborValue.Text(LOGIN),
        password: CborValue = CborValue.Text(PASSWORD),
        arguments: CborValue = CborValue.Array(emptyList()),
    ): ByteArray =
        CborEncoder.encode(
            CborValue.Array(
                listOf(
                    CborValue.UInt(operation),
                    login,
                    password,
                    arguments,
                ),
            ),
        )

    private fun node(
        senderId: ByteArray = SENDER_ID,
        label: String = LABEL,
        host: String = HOST,
        port: Int = PORT,
        x25519PublicKey: ByteArray = X25519_PUBLIC_KEY,
        ed25519PublicKey: ByteArray = ED25519_PUBLIC_KEY,
    ): CborValue =
        CborValue.Array(
            listOf(
                CborValue.Bytes(senderId),
                CborValue.Text(label),
                CborValue.Text(host),
                CborValue.UInt(port.toLong()),
                CborValue.Bytes(x25519PublicKey),
                CborValue.Bytes(ed25519PublicKey),
            ),
        )

    private fun topologyAnswer(vararg nodes: CborValue): ResultEnvelope =
        ResultEnvelope(ResultCodes.SUCCESS, listOf(CborValue.Array(nodes.toList())))

    private fun arrayItems(encoded: ByteArray): List<CborValue> = (CborDecoder.decode(encoded) as CborValue.Array).items

    private fun assertBytes(
        expected: ByteArray,
        actual: CborValue,
    ) = assertArrayEquals(expected, (actual as CborValue.Bytes).value)

    private companion object {
        const val PROTOCOL_VERSION = 1L
        const val OTHER_PROTOCOL_VERSION = 2L
        const val FIRST_FRAME_NUMBER = 0L
        const val FRAME_NUMBER = 7L
        const val UINT32_OVERFLOW = 4_294_967_296L
        const val TOPOLOGY_CODE = 2L
        const val UNKNOWN_OPERATION_ZERO = 0L
        const val UNKNOWN_OPERATION_FIVE = 5L
        const val NON_CONTRACT_RESULT_CODE = 1L
        const val PORT = 8_443
        const val OTHER_PORT = 9_443
        const val PORT_ZERO = 0
        const val PORT_ABOVE_RANGE = 65_536
        const val NON_TEXT_VALUE = 1L
        const val TWO_NODES = 2
        const val EXTERNAL_HEADER_ELEMENTS = 7
        const val SHORT_HEADER_ELEMENTS = 6
        const val CLIENT_LOGICAL_HEADER_ELEMENTS = 4
        const val RESULT_ENVELOPE_ELEMENTS = 2

        const val LABEL = "node-label"
        const val OTHER_LABEL = "other-label"
        const val HOST = "192.0.2.10"
        const val LOGIN = "operator"
        const val PASSWORD = "s3cret-pass"
        const val QUORUM_UNAVAILABLE_HEX = "0x00060001"

        val SENDER_ID = ByteArray(16) { index -> index.toByte() }
        val OTHER_SENDER_ID = ByteArray(16) { index -> (index + 16).toByte() }
        val REQUEST_ID = ByteArray(16) { index -> (index + 32).toByte() }
        val UPLOAD_ID = ByteArray(16) { index -> (index + 48).toByte() }
        val BOOTSTRAP_SIGNING_KEY = ByteArray(32) { index -> index.toByte() }
        val X25519_PUBLIC_KEY = ByteArray(32) { index -> (index + 32).toByte() }
        val ED25519_PUBLIC_KEY = ByteArray(32) { index -> (index + 64).toByte() }
        val SHORT_BYTE_STRING = ByteArray(15)
        val SHORT_BOOTSTRAP_SIGNING_KEY = ByteArray(31)
        val SHORT_PUBLIC_KEY = ByteArray(31)

        const val CONTRACT_SUCCESS = 0L
        const val CONTRACT_UNKNOWN_OPERATION = 0x0001_0001L
        const val CONTRACT_INVALID_ARGUMENTS = 0x0001_0002L
        const val CONTRACT_INVALID_REQUEST_STATE = 0x0001_0003L
        const val CONTRACT_REPEATED_REQUEST_ID = 0x0001_0004L
        const val CONTRACT_AUTHENTICATION_REJECTED = 0x0002_0001L
        const val CONTRACT_REGISTRATION_CONFLICT = 0x0003_0001L
        const val CONTRACT_REGISTRATION_REJECTED = 0x0003_0002L
        const val CONTRACT_INVALID_CURSOR = 0x0004_0001L
        const val CONTRACT_EXPIRED_SNAPSHOT = 0x0004_0002L
        const val CONTRACT_FILE_SIZE_LIMIT = 0x0005_0001L
        const val CONTRACT_SIZE_METADATA_OR_HASH_MISMATCH = 0x0005_0002L
        const val CONTRACT_INVALID_FRAME_SEQUENCE = 0x0005_0003L
        const val CONTRACT_UPLOAD_ID_CONFLICT = 0x0005_0004L
        const val CONTRACT_QUORUM_UNAVAILABLE = 0x0006_0001L
        const val CONTRACT_BLOB_QUORUM_NOT_REACHED = 0x0006_0002L
        const val CONTRACT_TEMPORARY_UNAVAILABLE = 0x0007_0001L
        const val CONTRACT_STORAGE_CAPACITY_EXHAUSTED = 0x0007_0002L
        const val CONTRACT_INTERNAL_ERROR = 0x0008_0001L
    }
}
