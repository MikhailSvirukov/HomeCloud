package dev.homecloud.android.topology

import dev.homecloud.android.crypto.CryptoException
import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.IdentityStorageException
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.ClientNetworkConfiguration
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.net.ProtocolTransportException
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.ProtocolEncodingException
import dev.homecloud.android.protocol.ResultEnvelopeCodec
import dev.homecloud.android.protocol.TopologyCodec
import dev.homecloud.android.protocol.TopologyNode
import java.io.IOException
import java.nio.ByteBuffer
import java.util.Locale
import java.util.UUID

/**
 * Reports that the storage topology could not be requested or read.
 *
 * Messages identify the failing topology request and never contain credentials, key material or
 * answer contents.
 */
internal class TopologyException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/**
 * Requests the storage topology from the dedicated server (`INT-NET-060 / 1`, `INT-NET-070 / 6`).
 *
 * The request uses operation `2` and the logical header `[2, login, password, []]` (`INT-NET-060`).
 * The frame is signed with the installation Ed25519 key, encrypted to the pinned dedicated-server
 * X25519 key, and carries the installation sender id, a fresh 16-byte request id, frame number zero,
 * and null upload id, responder label and bootstrap signing key (`INT-NET-025`, `INT-NET-034`,
 * `INT-NET-050`).
 *
 * The response must repeat the request id, verify against the pinned dedicated-server Ed25519 key,
 * and decrypt with the installation X25519 private key before its result envelope and topology
 * answer are read (`INT-NET-061`, `INT-NET-070`). Every returned node label is checked against the
 * configured UTF-8 byte limit (`CFG-CL-031`). The channel is closed when the attempt finishes.
 */
internal class TopologyClient(
    private val identityStore: InstallationIdentityStore,
    private val channelFactory: (host: String, port: Int) -> FrameChannel,
    private val maxFrameBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES,
    private val maxLabelBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_LABEL_BYTES,
) {
    /**
     * Requests the topology from the dedicated server at [host]:[port] and returns its nodes in
     * answer order (`INT-NET-070`).
     */
    fun fetch(
        host: String,
        port: Int,
        login: String,
        password: String,
    ): List<TopologyNode> =
        try {
            val identity = identityStore.load() ?: throw TopologyException(IDENTITY_UNAVAILABLE_MESSAGE)
            val senderId = identity.senderId
            if (senderId == null || senderId.size != SENDER_ID_BYTES || !identity.hasValidKeyPairs()) {
                throw TopologyException(IDENTITY_UNAVAILABLE_MESSAGE)
            }
            val serverKeys = identity.dedicatedServerKeys ?: throw TopologyException(PINNED_KEYS_UNAVAILABLE_MESSAGE)
            if (!serverKeys.hasValidKeyLengths()) {
                throw TopologyException(PINNED_KEYS_UNAVAILABLE_MESSAGE)
            }
            channelFactory(host, port).use { channel ->
                val requestId = newRequestId()
                channel.write(
                    encodeRequest(
                        login = login,
                        password = password,
                        senderId = senderId,
                        identity = identity,
                        serverKeys = serverKeys,
                        requestId = requestId,
                    ),
                )
                readAnswer(channel, identity, serverKeys, requestId)
            }
        } catch (exception: TopologyException) {
            throw exception
        } catch (exception: IOException) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolEncodingException) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: ProtocolTransportException) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: CryptoException) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: IdentityStorageException) {
            throw TopologyException(IDENTITY_UNAVAILABLE_MESSAGE, exception)
        } catch (exception: IllegalArgumentException) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        } catch (exception: Exception) {
            throw TopologyException(TOPOLOGY_REQUEST_FAILED_MESSAGE, exception)
        }

    private fun encodeRequest(
        login: String,
        password: String,
        senderId: ByteArray,
        identity: InstallationIdentity,
        serverKeys: DedicatedServerKeys,
        requestId: ByteArray,
    ): ByteArray =
        FrameBodyCodec.encode(
            header =
                ExternalFrameHeader(
                    senderId = senderId,
                    requestId = requestId,
                    frameNumber = 0,
                    uploadId = null,
                    responderLabel = null,
                    bootstrapSigningPublicKey = null,
                ),
            payload = FrameBodyCodec.encryptedPayload(serverKeys.x25519PublicKey, TopologyCodec.encodeRequest(login, password)),
            sign = { bytes -> Ed25519.sign(identity.signingKeyPair.privateKey, bytes) },
            maxFrameBytes = maxFrameBytes,
        )

    private fun readAnswer(
        channel: FrameChannel,
        identity: InstallationIdentity,
        serverKeys: DedicatedServerKeys,
        requestId: ByteArray,
    ): List<TopologyNode> {
        val response =
            FrameBodyCodec.decode(channel.read(), maxFrameBytes) { bytes, signature ->
                Ed25519.verify(serverKeys.ed25519PublicKey, bytes, signature)
            }
        if (!response.header.requestId.contentEquals(requestId)) {
            throw TopologyException(REQUEST_ID_MISMATCH_MESSAGE)
        }
        val envelope =
            ResultEnvelopeCodec.decode(
                FrameBodyCodec.decryptedPayload(identity.agreementKeyPair.privateKey, response.payload),
            )
        if (!envelope.isSuccess) {
            throw TopologyException(resultFailureMessage(envelope.resultCode))
        }
        val nodes = TopologyCodec.decodeAnswer(envelope)
        for (node in nodes) {
            if (node.label.toByteArray(Charsets.UTF_8).size > maxLabelBytes) {
                throw TopologyException(LABEL_TOO_LONG_MESSAGE)
            }
        }
        return nodes
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

private fun InstallationIdentity.hasValidKeyPairs(): Boolean =
    signingKeyPair.privateKey.size == KEY_BYTES &&
        signingKeyPair.publicKey.size == KEY_BYTES &&
        agreementKeyPair.privateKey.size == KEY_BYTES &&
        agreementKeyPair.publicKey.size == KEY_BYTES

private fun DedicatedServerKeys.hasValidKeyLengths(): Boolean = x25519PublicKey.size == KEY_BYTES && ed25519PublicKey.size == KEY_BYTES

private fun resultFailureMessage(resultCode: Long): String =
    "the topology request failed with result code 0x" +
        resultCode.toString(16).uppercase(Locale.ROOT).padStart(RESULT_CODE_HEX_DIGITS, '0')

private const val KEY_BYTES = 32
private const val SENDER_ID_BYTES = 16
private const val REQUEST_ID_BYTES = 16
private const val RESULT_CODE_HEX_DIGITS = 8

private const val IDENTITY_UNAVAILABLE_MESSAGE = "the installation identity is unavailable"
private const val PINNED_KEYS_UNAVAILABLE_MESSAGE = "the pinned dedicated server keys are unavailable"
private const val REQUEST_ID_MISMATCH_MESSAGE = "the topology response request id does not match the request"
private const val TOPOLOGY_REQUEST_FAILED_MESSAGE = "the topology request failed"
private const val LABEL_TOO_LONG_MESSAGE = "a topology node label exceeds the configured byte limit"
