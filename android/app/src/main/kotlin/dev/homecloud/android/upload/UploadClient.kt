package dev.homecloud.android.upload

import dev.homecloud.android.crypto.CryptoException
import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.identity.IdentityStorageException
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.ClientNetworkConfiguration
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.net.ProtocolTransportException
import dev.homecloud.android.payload.CompressedPayload
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
 * Reports that one version could not be uploaded to the addressed storage node.
 *
 * Messages identify the failing upload and never contain the login, password, key material,
 * metadata contents or payload bytes.
 */
internal class UploadException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/** One version to upload to one storage node. */
internal data class UploadRequest(
    val uploadId: ByteArray,
    val node: TopologyNode,
    val login: String,
    val password: String,
    val metadata: UploadMetadata,
    val payload: CompressedPayload,
)

/**
 * Uploads one compressed version directly to one storage node (`FR-UP-003`, `FR-UP-004`,
 * `FR-UP-005`, `FR-UP-006`, `FR-DED-015 / 1`, `INT-NET-057 / 1`, `INT-NET-078 / 1`).
 *
 * The request uses operation `4` and the logical header `[4, login, password,
 * [source_name, timestamp_ms, original_size, compressed_size, sha256]]`. Frame zero carries the
 * logical header followed by the first compressed bytes, and every later frame carries only the
 * next compressed bytes; frame numbers start at zero and increase by one without gaps
 * (`FR-UP-004`, `FR-UP-005`, `FR-UP-006`). Every frame is signed with the installation Ed25519 key
 * and encrypted to the node's X25519 key with a fresh HPKE encapsulation, and it carries the
 * assigned sender id, a fresh request id and the uploaded version's identifier
 * (`INT-NET-081 / 1`).
 *
 * The response must repeat the request id, verify against the node's Ed25519 key and decrypt with
 * the installation X25519 key. A successful envelope confirms exactly the uploaded 16-byte
 * identifier (`FR-UP-021`).
 *
 * An attempt keeps no partial state. When an attempt fails the caller retries the same upload
 * identifier with the same payload; every call uses a fresh request id and rebuilds the plan from
 * frame zero, so a retry restarts from frame zero (`FR-UP-016`, `CFG-CL-011`).
 */
internal class UploadClient(
    private val identityStore: InstallationIdentityStore,
    private val channelFactory: (host: String, port: Int) -> FrameChannel,
    private val metadataLimits: UploadMetadataLimits,
    private val maxFrameBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES,
) {
    fun upload(request: UploadRequest): ByteArray =
        try {
            val identity = identityStore.load() ?: throw UploadException(IDENTITY_UNAVAILABLE_MESSAGE)
            val senderId = identity.senderId
            if (senderId == null || senderId.size != SENDER_ID_BYTES || !identity.hasKeyPairs()) {
                throw UploadException(IDENTITY_UNAVAILABLE_MESSAGE)
            }
            validateRequest(request)
            val logicalHeader = encodeLogicalHeader(request)
            val requestId = newRequestId()
            val headerTemplate =
                ExternalFrameHeader(
                    senderId = senderId,
                    requestId = requestId,
                    frameNumber = 0,
                    uploadId = request.uploadId,
                    responderLabel = null,
                    bootstrapSigningPublicKey = null,
                )
            val plan = UploadFramePlanBuilder.plan(headerTemplate, logicalHeader, request.payload, maxFrameBytes)
            channelFactory(request.node.host, request.node.port).use { channel ->
                writeFrames(channel, request.node, identity, headerTemplate, plan)
                readConfirmedUploadId(channel, request.node, identity, requestId, request.uploadId)
            }
        } catch (exception: UploadException) {
            throw exception
        } catch (exception: IOException) {
            throw UploadException(UPLOAD_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolTransportException) {
            throw UploadException(UPLOAD_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolEncodingException) {
            throw UploadException(UPLOAD_FAILED_MESSAGE, exception)
        } catch (exception: CryptoException) {
            throw UploadException(UPLOAD_FAILED_MESSAGE, exception)
        } catch (exception: IdentityStorageException) {
            throw UploadException(IDENTITY_UNAVAILABLE_MESSAGE, exception)
        } catch (exception: Exception) {
            throw UploadException(UPLOAD_FAILED_MESSAGE, exception)
        }

    private fun validateRequest(request: UploadRequest) {
        if (request.uploadId.size != UPLOAD_ID_BYTES) {
            throw UploadException("the upload id must contain $UPLOAD_ID_BYTES bytes")
        }
        if (request.metadata.compressedSizeBytes != request.payload.totalBytes) {
            throw UploadException(COMPRESSED_SIZE_MISMATCH_MESSAGE)
        }
        if (maxFrameBytes <= 0) {
            throw UploadException(FRAME_LIMIT_MESSAGE)
        }
    }

    private fun encodeLogicalHeader(request: UploadRequest): ByteArray {
        val arguments =
            try {
                UploadMetadataCodec.encode(request.metadata, metadataLimits)
            } catch (exception: IllegalArgumentException) {
                throw UploadException(METADATA_REJECTED_MESSAGE, exception)
            }
        return ClientLogicalHeaderCodec.encode(
            ClientLogicalHeader(
                operation = OperationType.UPLOAD,
                login = request.login,
                password = request.password,
                arguments = arguments,
            ),
        )
    }

    private fun writeFrames(
        channel: FrameChannel,
        node: TopologyNode,
        identity: InstallationIdentity,
        headerTemplate: ExternalFrameHeader,
        plan: UploadFramePlan,
    ) {
        for (index in 0 until plan.frameCount) {
            channel.write(
                FrameBodyCodec.encode(
                    header = headerTemplate.copy(frameNumber = index.toLong()),
                    payload = FrameBodyCodec.encryptedPayload(node.x25519PublicKey, plan.frame(index)),
                    sign = { bytes -> Ed25519.sign(identity.signingKeyPair.privateKey, bytes) },
                    maxFrameBytes = maxFrameBytes,
                ),
            )
        }
    }

    private fun readConfirmedUploadId(
        channel: FrameChannel,
        node: TopologyNode,
        identity: InstallationIdentity,
        requestId: ByteArray,
        uploadId: ByteArray,
    ): ByteArray {
        val response =
            FrameBodyCodec.decode(channel.read(), maxFrameBytes) { bytes, signature ->
                Ed25519.verify(node.ed25519PublicKey, bytes, signature)
            }
        if (!response.header.requestId.contentEquals(requestId)) {
            throw UploadException(REQUEST_ID_MISMATCH_MESSAGE)
        }
        val envelope =
            ResultEnvelopeCodec.decode(
                FrameBodyCodec.decryptedPayload(identity.agreementKeyPair.privateKey, response.payload),
            )
        return confirmedUploadId(envelope, uploadId)
    }

    private fun confirmedUploadId(
        envelope: ResultEnvelope,
        uploadId: ByteArray,
    ): ByteArray {
        if (!envelope.isSuccess) {
            throw UploadException(resultFailureMessage(envelope.resultCode))
        }
        if (envelope.arguments.size != SUCCESS_ARGUMENT_COUNT) {
            throw UploadException(CONFIRMATION_SHAPE_MESSAGE)
        }
        val confirmed =
            (envelope.arguments[0] as? CborValue.Bytes)?.value
                ?: throw UploadException(CONFIRMATION_SHAPE_MESSAGE)
        if (confirmed.size != UPLOAD_ID_BYTES) {
            throw UploadException(CONFIRMATION_SHAPE_MESSAGE)
        }
        if (!confirmed.contentEquals(uploadId)) {
            throw UploadException(CONFIRMATION_MISMATCH_MESSAGE)
        }
        return confirmed
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

private fun resultFailureMessage(resultCode: Long): String =
    "the upload request failed with result code 0x" +
        resultCode.toString(16).uppercase(Locale.ROOT).padStart(RESULT_CODE_HEX_DIGITS, '0')

private const val KEY_BYTES = 32
private const val SENDER_ID_BYTES = 16
private const val REQUEST_ID_BYTES = 16
private const val UPLOAD_ID_BYTES = 16
private const val SUCCESS_ARGUMENT_COUNT = 1
private const val RESULT_CODE_HEX_DIGITS = 8

private const val IDENTITY_UNAVAILABLE_MESSAGE = "the installation identity is unavailable"
private const val UPLOAD_FAILED_MESSAGE = "the upload request failed"
private const val COMPRESSED_SIZE_MISMATCH_MESSAGE =
    "the declared compressed size does not match the payload"
private const val FRAME_LIMIT_MESSAGE = "the configured maximum frame size must be positive"
private const val METADATA_REJECTED_MESSAGE = "the upload metadata is not acceptable"
private const val REQUEST_ID_MISMATCH_MESSAGE = "the upload response request id does not match the request"
private const val CONFIRMATION_SHAPE_MESSAGE = "the upload response does not confirm exactly one 16-byte upload id"
private const val CONFIRMATION_MISMATCH_MESSAGE = "the upload response confirms a different upload id"
