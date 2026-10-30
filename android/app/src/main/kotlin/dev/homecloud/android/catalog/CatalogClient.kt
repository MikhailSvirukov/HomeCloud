package dev.homecloud.android.catalog

import dev.homecloud.android.crypto.CryptoException
import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.identity.IdentityStorageException
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.ClientNetworkConfiguration
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.net.ProtocolTransportException
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ClientLogicalHeader
import dev.homecloud.android.protocol.ClientLogicalHeaderCodec
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.OperationType
import dev.homecloud.android.protocol.ProtocolEncodingException
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import dev.homecloud.android.protocol.TopologyNode
import java.io.IOException
import java.nio.ByteBuffer
import java.util.Locale
import java.util.UUID

/**
 * Reports that one catalog page could not be requested or read.
 *
 * Messages identify the failing operation and never contain the password, key material, catalog
 * contents or other request data.
 */
internal class CatalogException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/**
 * One entry of a catalog page (`FR-CAT-012 / 1`).
 *
 * The upload identifier and the SHA-256 digest have their fixed wire lengths, and the two size
 * fields are non-negative.
 */
internal data class CatalogEntry(
    val uploadId: ByteArray,
    val sourceName: String,
    val timestampMillis: Long,
    val originalSizeBytes: Long,
    val compressedSizeBytes: Long,
    val sha256: ByteArray,
)

/**
 * One page of a storage node's catalog (`FR-CAT-013 / 1`).
 *
 * The snapshot token is present on every page. A next cursor is present only while a further page
 * exists, and the client sends both values back unchanged to continue the same snapshot
 * (`FR-CAT-009 / 1`, `FR-CAT-014 / 1`).
 */
internal data class CatalogPage(
    val snapshotToken: ByteArray,
    val nextCursor: ByteArray?,
    val entries: List<CatalogEntry>,
)

/**
 * Requests one page of a storage node's catalog over a direct client connection (`INT-NET-080 / 1`).
 *
 * The request uses operation `3` and the logical header `[3, login, password, [snapshot_token,
 * cursor]]`, with both arguments `null` for the first page and the unchanged 16-byte values for a
 * continuation page (`FR-CAT-015 / 1`, `INT-NET-084 / 1`). The frame is addressed to the storage
 * node's topology host and port, signed with the installation Ed25519 key, encrypted to the node's
 * X25519 key, and carries the assigned sender id, a fresh request id, frame number zero, and null
 * upload id, responder label and bootstrap signing key (`INT-NET-050 / 4`, `INT-NET-080 / 1`).
 *
 * The response must repeat the request id, verify against the node's Ed25519 key, and decrypt with
 * the installation X25519 key before its result envelope is read. A successful envelope carries
 * the catalog page array, and each entry is validated against its exact positional shape
 * (`FR-CAT-012 / 1`, `FR-CAT-013 / 1`, `INT-NET-061 / 1`). The channel is closed when the attempt
 * finishes.
 */
internal class CatalogClient(
    private val identityStore: InstallationIdentityStore,
    private val channelFactory: (host: String, port: Int) -> FrameChannel,
    private val maxFrameBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES,
) {
    fun fetchPage(
        node: TopologyNode,
        login: String,
        password: String,
        snapshotToken: ByteArray?,
        cursor: ByteArray?,
    ): CatalogPage =
        try {
            val identity = identityStore.load() ?: throw CatalogException(IDENTITY_UNAVAILABLE_MESSAGE)
            val senderId = identity.senderId
            if (senderId == null || senderId.size != SENDER_ID_BYTES || !identity.hasKeyPairs()) {
                throw CatalogException(IDENTITY_UNAVAILABLE_MESSAGE)
            }
            requireToken(snapshotToken, SNAPSHOT_TOKEN_FIELD)
            requireToken(cursor, CURSOR_FIELD)
            channelFactory(node.host, node.port).use { channel ->
                val requestId = newRequestId()
                channel.write(
                    encodeRequest(
                        node = node,
                        login = login,
                        password = password,
                        snapshotToken = snapshotToken,
                        cursor = cursor,
                        senderId = senderId,
                        identity = identity,
                        requestId = requestId,
                    ),
                )
                readPage(channel, node, identity, requestId)
            }
        } catch (exception: CatalogException) {
            throw exception
        } catch (exception: IOException) {
            throw CatalogException(CATALOG_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolEncodingException) {
            throw CatalogException(CATALOG_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolTransportException) {
            throw CatalogException(CATALOG_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: CryptoException) {
            throw CatalogException(CATALOG_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: IdentityStorageException) {
            throw CatalogException(IDENTITY_UNAVAILABLE_MESSAGE, exception)
        } catch (exception: Exception) {
            throw CatalogException(CATALOG_REQUEST_FAILED_MESSAGE, exception)
        }

    private fun encodeRequest(
        node: TopologyNode,
        login: String,
        password: String,
        snapshotToken: ByteArray?,
        cursor: ByteArray?,
        senderId: ByteArray,
        identity: InstallationIdentity,
        requestId: ByteArray,
    ): ByteArray {
        val logicalHeader =
            ClientLogicalHeaderCodec.encode(
                ClientLogicalHeader(
                    operation = OperationType.CATALOG,
                    login = login,
                    password = password,
                    arguments =
                        listOf(
                            snapshotToken.toCborToken(),
                            cursor.toCborToken(),
                        ),
                ),
            )
        return FrameBodyCodec.encode(
            header =
                ExternalFrameHeader(
                    senderId = senderId,
                    requestId = requestId,
                    frameNumber = 0,
                    uploadId = null,
                    responderLabel = null,
                    bootstrapSigningPublicKey = null,
                ),
            payload = FrameBodyCodec.encryptedPayload(node.x25519PublicKey, logicalHeader),
            sign = { bytes -> Ed25519.sign(identity.signingKeyPair.privateKey, bytes) },
            maxFrameBytes = maxFrameBytes,
        )
    }

    private fun readPage(
        channel: FrameChannel,
        node: TopologyNode,
        identity: InstallationIdentity,
        requestId: ByteArray,
    ): CatalogPage {
        val response =
            FrameBodyCodec.decode(channel.read(), maxFrameBytes) { bytes, signature ->
                Ed25519.verify(node.ed25519PublicKey, bytes, signature)
            }
        if (!response.header.requestId.contentEquals(requestId)) {
            throw CatalogException(REQUEST_ID_MISMATCH_MESSAGE)
        }
        val envelope =
            ResultEnvelopeCodec.decode(
                FrameBodyCodec.decryptedPayload(identity.agreementKeyPair.privateKey, response.payload),
            )
        return decodePage(envelope)
    }

    private fun decodePage(envelope: ResultEnvelope): CatalogPage {
        if (!envelope.isSuccess) {
            throw CatalogException(resultFailureMessage(envelope.resultCode))
        }
        if (envelope.arguments.size != CATALOG_ARGUMENT_COUNT) {
            throw CatalogException(
                "the catalog page must contain exactly $CATALOG_ARGUMENT_COUNT arguments",
            )
        }
        val snapshotToken = envelope.arguments[0].byteString(SNAPSHOT_TOKEN_FIELD, TOKEN_BYTES)
        val nextCursor = envelope.arguments[1].optionalByteString(CURSOR_FIELD, TOKEN_BYTES)
        val entries = envelope.arguments[2].asArray(ENTRIES_FIELD).map { entry -> decodeEntry(entry) }
        return CatalogPage(
            snapshotToken = snapshotToken,
            nextCursor = nextCursor,
            entries = entries,
        )
    }

    private fun decodeEntry(value: CborValue): CatalogEntry {
        val elements = value.asArray(ENTRY_FIELD)
        if (elements.size != ENTRY_ELEMENT_COUNT) {
            throw CatalogException(
                "a catalog entry must contain exactly $ENTRY_ELEMENT_COUNT elements",
            )
        }
        return CatalogEntry(
            uploadId = elements[0].byteString(UPLOAD_ID_FIELD, UPLOAD_ID_BYTES),
            sourceName = elements[1].text(SOURCE_NAME_FIELD),
            timestampMillis = elements[2].signedInteger(TIMESTAMP_FIELD),
            originalSizeBytes = elements[3].nonNegativeInteger(ORIGINAL_SIZE_FIELD),
            compressedSizeBytes = elements[4].nonNegativeInteger(COMPRESSED_SIZE_FIELD),
            sha256 = elements[5].byteString(SHA256_FIELD, SHA256_BYTES),
        )
    }

    private fun newRequestId(): ByteArray {
        val uuid = UUID.randomUUID()
        return ByteBuffer
            .allocate(REQUEST_ID_BYTES)
            .putLong(uuid.mostSignificantBits)
            .putLong(uuid.leastSignificantBits)
            .array()
    }
}

private fun InstallationIdentity.hasKeyPairs(): Boolean =
    signingKeyPair.privateKey.size == KEY_BYTES &&
        signingKeyPair.publicKey.size == KEY_BYTES &&
        agreementKeyPair.privateKey.size == KEY_BYTES &&
        agreementKeyPair.publicKey.size == KEY_BYTES

private fun requireToken(
    value: ByteArray?,
    field: String,
) {
    if (value != null && value.size != TOKEN_BYTES) {
        throw CatalogException("$field must contain $TOKEN_BYTES bytes")
    }
}

private fun ByteArray?.toCborToken(): CborValue = this?.let { CborValue.Bytes(it) } ?: CborValue.Null

private fun CborValue.byteString(
    field: String,
    expectedLength: Int,
): ByteArray {
    val bytes =
        (this as? CborValue.Bytes)?.value
            ?: throw CatalogException("$field must be a byte string")
    if (bytes.size != expectedLength) {
        throw CatalogException("$field must contain $expectedLength bytes")
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

private fun CborValue.text(field: String): String =
    (this as? CborValue.Text)?.value
        ?: throw CatalogException("$field must be a text string")

private fun CborValue.signedInteger(field: String): Long =
    when (this) {
        is CborValue.UInt -> value
        is CborValue.NInt -> value
        else -> throw CatalogException("$field must be an integer")
    }

private fun CborValue.nonNegativeInteger(field: String): Long =
    (this as? CborValue.UInt)?.value
        ?: throw CatalogException("$field must be a non-negative integer")

private fun CborValue.asArray(field: String): List<CborValue> =
    (this as? CborValue.Array)?.items
        ?: throw CatalogException("$field must be an array")

private fun resultFailureMessage(resultCode: Long): String =
    "the catalog request failed with result code 0x" +
        resultCode.toString(16).uppercase(Locale.ROOT).padStart(RESULT_CODE_HEX_DIGITS, '0')

private const val KEY_BYTES = 32
private const val SENDER_ID_BYTES = 16
private const val REQUEST_ID_BYTES = 16
private const val TOKEN_BYTES = 16
private const val UPLOAD_ID_BYTES = 16
private const val SHA256_BYTES = 32
private const val CATALOG_ARGUMENT_COUNT = 3
private const val ENTRY_ELEMENT_COUNT = 6
private const val RESULT_CODE_HEX_DIGITS = 8

private const val SNAPSHOT_TOKEN_FIELD = "catalog snapshot token"
private const val CURSOR_FIELD = "catalog cursor"
private const val ENTRIES_FIELD = "catalog entries"
private const val ENTRY_FIELD = "catalog entry"
private const val UPLOAD_ID_FIELD = "catalog upload id"
private const val SOURCE_NAME_FIELD = "catalog source name"
private const val TIMESTAMP_FIELD = "catalog timestamp"
private const val ORIGINAL_SIZE_FIELD = "catalog original size"
private const val COMPRESSED_SIZE_FIELD = "catalog compressed size"
private const val SHA256_FIELD = "catalog sha256"

private const val IDENTITY_UNAVAILABLE_MESSAGE = "the installation identity is unavailable"
private const val REQUEST_ID_MISMATCH_MESSAGE = "the catalog response request id does not match the request"
private const val CATALOG_REQUEST_FAILED_MESSAGE = "the catalog page request failed"
