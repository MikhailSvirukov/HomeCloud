package dev.homecloud.android.protocol

import java.util.Locale

/**
 * The external frame header of the v1 protocol (`INT-NET-006`, `INT-NET-008`, `INT-NET-047`,
 * `INT-NET-049`, `INT-NET-050`).
 *
 * Every message carries all seven positional elements in contract order. Optional elements are
 * null in their own position, and the sender id is null only for primary registration before an
 * identifier is assigned.
 */
internal data class ExternalFrameHeader(
    val senderId: ByteArray?,
    val requestId: ByteArray,
    val frameNumber: Long,
    val uploadId: ByteArray?,
    val responderLabel: String?,
    val bootstrapSigningPublicKey: ByteArray?,
)

/**
 * Encodes and decodes the positional external frame header (`INT-NET-006`, `INT-NET-008`,
 * `INT-NET-047`, `INT-NET-049`, `INT-NET-050`).
 *
 * Only protocol version 1 is accepted. Byte strings are checked against the fixed contract lengths
 * and the frame number is restricted to the unsigned 32-bit range.
 */
internal object ExternalFrameHeaderCodec {
    fun encode(header: ExternalFrameHeader): ByteArray {
        header.senderId?.let { requireByteLength(it, SENDER_ID_BYTES, SENDER_ID_FIELD) }
        requireByteLength(header.requestId, REQUEST_ID_BYTES, REQUEST_ID_FIELD)
        if (header.frameNumber !in uint32Range) {
            throw ProtocolEncodingException("$FRAME_NUMBER_FIELD is out of range")
        }
        header.uploadId?.let { requireByteLength(it, UPLOAD_ID_BYTES, UPLOAD_ID_FIELD) }
        header.bootstrapSigningPublicKey?.let {
            requireByteLength(it, BOOTSTRAP_SIGNING_PUBLIC_KEY_BYTES, BOOTSTRAP_SIGNING_PUBLIC_KEY_FIELD)
        }
        val elements =
            listOf(
                CborValue.UInt(PROTOCOL_VERSION),
                header.senderId.toCborBytes(),
                CborValue.Bytes(header.requestId),
                CborValue.UInt(header.frameNumber),
                header.uploadId.toCborBytes(),
                header.responderLabel?.let { CborValue.Text(it) } ?: CborValue.Null,
                header.bootstrapSigningPublicKey.toCborBytes(),
            )
        return CborEncoder.encode(CborValue.Array(elements))
    }

    fun decode(bytes: ByteArray): ExternalFrameHeader {
        val elements = CborDecoder.decode(bytes).asArray(EXTERNAL_FRAME_HEADER_FIELD)
        if (elements.size != EXTERNAL_HEADER_ELEMENT_COUNT) {
            throw ProtocolEncodingException(
                "$EXTERNAL_FRAME_HEADER_FIELD must contain exactly $EXTERNAL_HEADER_ELEMENT_COUNT elements",
            )
        }
        val version = elements[0].unsignedInteger(PROTOCOL_VERSION_FIELD, uint32Range)
        if (version != PROTOCOL_VERSION) {
            throw ProtocolEncodingException("unsupported protocol version")
        }
        return ExternalFrameHeader(
            senderId = elements[1].optionalByteString(SENDER_ID_FIELD, SENDER_ID_BYTES),
            requestId = elements[2].byteString(REQUEST_ID_FIELD, REQUEST_ID_BYTES),
            frameNumber = elements[3].unsignedInteger(FRAME_NUMBER_FIELD, uint32Range),
            uploadId = elements[4].optionalByteString(UPLOAD_ID_FIELD, UPLOAD_ID_BYTES),
            responderLabel = elements[5].optionalText(RESPONDER_LABEL_FIELD),
            bootstrapSigningPublicKey =
                elements[6].optionalByteString(
                    BOOTSTRAP_SIGNING_PUBLIC_KEY_FIELD,
                    BOOTSTRAP_SIGNING_PUBLIC_KEY_BYTES,
                ),
        )
    }
}

/**
 * The client operation codes fixed by the v1 operation table (`INT-NET-057`).
 *
 * Code `0` is invalid, registration is `1`, topology retrieval is `2`, a catalog-page request is
 * `3` and a media upload is `4`.
 */
internal enum class OperationType(
    val code: Long,
) {
    REGISTRATION(1),
    TOPOLOGY(2),
    CATALOG(3),
    UPLOAD(4),
}

/**
 * The client logical header prefixed to every authenticated client request (`INT-NET-047`,
 * `INT-NET-049`, `INT-NET-051`, `INT-NET-054`, `INT-NET-055`, `INT-NET-060`).
 *
 * The login and password text strings are carried exactly as supplied, without trimming or Unicode
 * normalization, and the arguments array schema follows from the operation type.
 */
internal data class ClientLogicalHeader(
    val operation: OperationType,
    val login: String,
    val password: String,
    val arguments: List<CborValue>,
)

/**
 * Encodes and decodes the positional client logical header (`INT-NET-047`, `INT-NET-049`,
 * `INT-NET-055`, `INT-NET-057`).
 *
 * The operation is a minimal unsigned integer and the array has exactly the four contract
 * positions. Unknown operation codes and non-array arguments are rejected.
 */
internal object ClientLogicalHeaderCodec {
    fun encode(header: ClientLogicalHeader): ByteArray {
        val elements =
            listOf(
                CborValue.UInt(header.operation.code),
                CborValue.Text(header.login),
                CborValue.Text(header.password),
                CborValue.Array(header.arguments),
            )
        return CborEncoder.encode(CborValue.Array(elements))
    }

    fun decode(bytes: ByteArray): ClientLogicalHeader {
        val elements = CborDecoder.decode(bytes).asArray(CLIENT_LOGICAL_HEADER_FIELD)
        if (elements.size != CLIENT_LOGICAL_HEADER_ELEMENT_COUNT) {
            throw ProtocolEncodingException(
                "$CLIENT_LOGICAL_HEADER_FIELD must contain exactly $CLIENT_LOGICAL_HEADER_ELEMENT_COUNT elements",
            )
        }
        val code = elements[0].unsignedInteger(OPERATION_FIELD, uint32Range)
        val operation =
            OperationType.entries.firstOrNull { candidate -> candidate.code == code }
                ?: throw ProtocolEncodingException("unknown operation code")
        return ClientLogicalHeader(
            operation = operation,
            login = elements[1].text(LOGIN_FIELD),
            password = elements[2].text(PASSWORD_FIELD),
            arguments = elements[3].asArray(ARGUMENTS_FIELD),
        )
    }
}

/**
 * The result envelope of a server response to a client request (`INT-NET-061`, `INT-NET-062`,
 * `INT-NET-063`).
 *
 * The result code is `0` for successful execution and non-zero for every error.
 */
internal data class ResultEnvelope(
    val resultCode: Long,
    val arguments: List<CborValue>,
) {
    /** Reports whether the envelope carries the successful result code. */
    val isSuccess: Boolean
        get() = resultCode == ResultCodes.SUCCESS
}

/**
 * Encodes and decodes the positional result envelope (`INT-NET-061`, `INT-NET-063`).
 *
 * The result code is restricted to the unsigned 32-bit range and the array has exactly the two
 * contract positions.
 */
internal object ResultEnvelopeCodec {
    fun encode(envelope: ResultEnvelope): ByteArray {
        if (envelope.resultCode !in uint32Range) {
            throw ProtocolEncodingException("$RESULT_CODE_FIELD is out of range")
        }
        val elements =
            listOf(
                CborValue.UInt(envelope.resultCode),
                CborValue.Array(envelope.arguments),
            )
        return CborEncoder.encode(CborValue.Array(elements))
    }

    fun decode(bytes: ByteArray): ResultEnvelope {
        val elements = CborDecoder.decode(bytes).asArray(RESULT_ENVELOPE_FIELD)
        if (elements.size != RESULT_ENVELOPE_ELEMENT_COUNT) {
            throw ProtocolEncodingException(
                "$RESULT_ENVELOPE_FIELD must contain exactly $RESULT_ENVELOPE_ELEMENT_COUNT elements",
            )
        }
        return ResultEnvelope(
            resultCode = elements[0].unsignedInteger(RESULT_CODE_FIELD, uint32Range),
            arguments = elements[1].asArray(RESULT_ARGUMENTS_FIELD),
        )
    }
}

/**
 * One storage-node entry of the topology answer (`INT-NET-070`).
 *
 * The label and host are text strings without Unicode normalization, and the sender identifier and
 * both public keys have the fixed contract lengths.
 */
internal data class TopologyNode(
    val senderId: ByteArray,
    val label: String,
    val host: String,
    val port: Int,
    val x25519PublicKey: ByteArray,
    val ed25519PublicKey: ByteArray,
)

/**
 * Builds topology requests and reads topology answers (`INT-NET-060`, `INT-NET-070`).
 *
 * The request is the topology operation with an empty argument array. The answer must be successful
 * and carry one array of node arrays, each with the six contract positions.
 */
internal object TopologyCodec {
    fun encodeRequest(
        login: String,
        password: String,
    ): ByteArray =
        ClientLogicalHeaderCodec.encode(
            ClientLogicalHeader(
                operation = OperationType.TOPOLOGY,
                login = login,
                password = password,
                arguments = emptyList(),
            ),
        )

    fun decodeAnswer(envelope: ResultEnvelope): List<TopologyNode> {
        if (!envelope.isSuccess) {
            throw ProtocolEncodingException(
                "topology request failed with result code ${hexadecimal(envelope.resultCode)}",
            )
        }
        if (envelope.arguments.size != TOPOLOGY_ARGUMENT_COUNT) {
            throw ProtocolEncodingException(
                "topology answer must contain exactly $TOPOLOGY_ARGUMENT_COUNT argument",
            )
        }
        return envelope.arguments[0].asArray(TOPOLOGY_NODES_FIELD).map { node -> decodeNode(node) }
    }

    private fun decodeNode(value: CborValue): TopologyNode {
        val elements = value.asArray(TOPOLOGY_NODE_FIELD)
        if (elements.size != TOPOLOGY_NODE_ELEMENT_COUNT) {
            throw ProtocolEncodingException(
                "$TOPOLOGY_NODE_FIELD must contain exactly $TOPOLOGY_NODE_ELEMENT_COUNT elements",
            )
        }
        return TopologyNode(
            senderId = elements[0].byteString(TOPOLOGY_SENDER_ID_FIELD, SENDER_ID_BYTES),
            label = elements[1].text(TOPOLOGY_LABEL_FIELD),
            host = elements[2].text(TOPOLOGY_HOST_FIELD),
            port = elements[3].unsignedInteger(TOPOLOGY_PORT_FIELD, portRange).toInt(),
            x25519PublicKey = elements[4].byteString(TOPOLOGY_X25519_PUBLIC_KEY_FIELD, PUBLIC_KEY_BYTES),
            ed25519PublicKey = elements[5].byteString(TOPOLOGY_ED25519_PUBLIC_KEY_FIELD, PUBLIC_KEY_BYTES),
        )
    }
}

/**
 * The published v1 result codes (`INT-NET-063`).
 *
 * Zero reports success; every other value is an error from the reserved range class named by its
 * high-order bytes.
 */
internal object ResultCodes {
    const val SUCCESS = 0L
    const val UNKNOWN_OPERATION = 0x0001_0001L
    const val INVALID_ARGUMENTS = 0x0001_0002L
    const val INVALID_REQUEST_STATE = 0x0001_0003L
    const val REPEATED_REQUEST_ID = 0x0001_0004L
    const val AUTHENTICATION_REJECTED = 0x0002_0001L
    const val REGISTRATION_CONFLICT = 0x0003_0001L
    const val REGISTRATION_REJECTED = 0x0003_0002L
    const val INVALID_CURSOR = 0x0004_0001L
    const val EXPIRED_SNAPSHOT = 0x0004_0002L
    const val FILE_SIZE_LIMIT = 0x0005_0001L
    const val SIZE_METADATA_OR_HASH_MISMATCH = 0x0005_0002L
    const val INVALID_FRAME_SEQUENCE = 0x0005_0003L
    const val UPLOAD_ID_CONFLICT = 0x0005_0004L
    const val QUORUM_UNAVAILABLE = 0x0006_0001L
    const val BLOB_QUORUM_NOT_REACHED = 0x0006_0002L
    const val TEMPORARY_UNAVAILABLE = 0x0007_0001L
    const val STORAGE_CAPACITY_EXHAUSTED = 0x0007_0002L
    const val INTERNAL_ERROR = 0x0008_0001L
}

private fun requireByteLength(
    value: ByteArray,
    expectedLength: Int,
    field: String,
) {
    if (value.size != expectedLength) {
        throw ProtocolEncodingException("$field must contain $expectedLength bytes")
    }
}

private fun ByteArray?.toCborBytes(): CborValue = this?.let { CborValue.Bytes(it) } ?: CborValue.Null

private fun CborValue.byteString(
    field: String,
    expectedLength: Int,
): ByteArray {
    val bytes =
        (this as? CborValue.Bytes)?.value
            ?: throw ProtocolEncodingException("$field must be a byte string")
    if (bytes.size != expectedLength) {
        throw ProtocolEncodingException("$field must contain $expectedLength bytes")
    }
    return bytes
}

private fun CborValue.optionalByteString(
    field: String,
    expectedLength: Int,
): ByteArray? =
    if (this is CborValue.Null) {
        null
    } else {
        byteString(field, expectedLength)
    }

private fun CborValue.unsignedInteger(
    field: String,
    range: LongRange,
): Long {
    val value =
        (this as? CborValue.UInt)?.value
            ?: throw ProtocolEncodingException("$field must be an unsigned integer")
    if (value !in range) {
        throw ProtocolEncodingException("$field is out of range")
    }
    return value
}

private fun CborValue.text(field: String): String =
    (this as? CborValue.Text)?.value
        ?: throw ProtocolEncodingException("$field must be a text string")

private fun CborValue.optionalText(field: String): String? =
    if (this is CborValue.Null) {
        null
    } else {
        text(field)
    }

private fun CborValue.asArray(field: String): List<CborValue> =
    (this as? CborValue.Array)?.items
        ?: throw ProtocolEncodingException("$field must be an array")

private fun hexadecimal(code: Long): String = "0x" + code.toString(16).uppercase(Locale.ROOT).padStart(RESULT_CODE_HEX_DIGITS, '0')

private const val PROTOCOL_VERSION = 1L
private const val UINT32_MAX = 4_294_967_295L
private val uint32Range = 0L..UINT32_MAX
private val portRange = 1L..65535L

private const val EXTERNAL_HEADER_ELEMENT_COUNT = 7
private const val CLIENT_LOGICAL_HEADER_ELEMENT_COUNT = 4
private const val RESULT_ENVELOPE_ELEMENT_COUNT = 2
private const val TOPOLOGY_ARGUMENT_COUNT = 1
private const val TOPOLOGY_NODE_ELEMENT_COUNT = 6
private const val RESULT_CODE_HEX_DIGITS = 8

private const val SENDER_ID_BYTES = 16
private const val REQUEST_ID_BYTES = 16
private const val UPLOAD_ID_BYTES = 16
private const val BOOTSTRAP_SIGNING_PUBLIC_KEY_BYTES = 32
private const val PUBLIC_KEY_BYTES = 32

private const val EXTERNAL_FRAME_HEADER_FIELD = "external frame header"
private const val PROTOCOL_VERSION_FIELD = "protocol version"
private const val SENDER_ID_FIELD = "sender id"
private const val REQUEST_ID_FIELD = "request id"
private const val FRAME_NUMBER_FIELD = "frame number"
private const val UPLOAD_ID_FIELD = "upload id"
private const val RESPONDER_LABEL_FIELD = "responder label"
private const val BOOTSTRAP_SIGNING_PUBLIC_KEY_FIELD = "bootstrap signing public key"
private const val CLIENT_LOGICAL_HEADER_FIELD = "client logical header"
private const val OPERATION_FIELD = "operation"
private const val LOGIN_FIELD = "login"
private const val PASSWORD_FIELD = "password"
private const val ARGUMENTS_FIELD = "arguments"
private const val RESULT_ENVELOPE_FIELD = "result envelope"
private const val RESULT_CODE_FIELD = "result code"
private const val RESULT_ARGUMENTS_FIELD = "result arguments"
private const val TOPOLOGY_NODES_FIELD = "topology nodes"
private const val TOPOLOGY_NODE_FIELD = "topology node"
private const val TOPOLOGY_SENDER_ID_FIELD = "topology sender id"
private const val TOPOLOGY_LABEL_FIELD = "topology label"
private const val TOPOLOGY_HOST_FIELD = "topology host"
private const val TOPOLOGY_PORT_FIELD = "topology port"
private const val TOPOLOGY_X25519_PUBLIC_KEY_FIELD = "topology x25519 public key"
private const val TOPOLOGY_ED25519_PUBLIC_KEY_FIELD = "topology ed25519 public key"
