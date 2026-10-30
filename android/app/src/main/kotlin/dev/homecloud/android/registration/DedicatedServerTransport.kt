package dev.homecloud.android.registration

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ClientLogicalHeader
import dev.homecloud.android.protocol.ClientLogicalHeaderCodec
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.FrameBodyCodec
import dev.homecloud.android.protocol.OperationType
import dev.homecloud.android.protocol.ResultEnvelope
import dev.homecloud.android.protocol.ResultEnvelopeCodec

/**
 * Performs the dedicated server's first-contact bootstrap handshake and sends one primary
 * registration frame over a freshly opened channel (`INT-NET-021`, `INT-NET-024`, `INT-NET-025`,
 * `INT-NET-026`, `INT-NET-034`, `INT-NET-036`, `INT-NET-050`, `INT-NET-058`, `INT-NET-059`,
 * `INT-NET-081`, `FR-AND-052`).
 *
 * The channel is closed when the attempt finishes. The installation identity supplies the signing
 * and agreement key pairs, and the dedicated server keys are pinned on first contact, so a later
 * mismatch ends the attempt. The response signature is verified before its payload is decrypted.
 * The password and key material are never logged or placed into an exception message
 * (`FR-AND-023`).
 */
internal class DedicatedServerTransport(
    private val identityStore: InstallationIdentityStore,
    private val channelFactory: () -> FrameChannel,
    private val maxFrameBytes: Int,
) : RegistrationTransport {
    override fun register(request: RegistrationRequest): RegistrationResponse =
        try {
            val identity =
                identityStore.load()
                    ?: throw RegistrationTransportException(IDENTITY_UNAVAILABLE_MESSAGE)
            if (!identity.hasKeyMaterial()) {
                throw RegistrationTransportException(IDENTITY_UNAVAILABLE_MESSAGE)
            }
            if (!request.x25519PublicKey.contentEquals(identity.agreementKeyPair.publicKey) ||
                !request.ed25519PublicKey.contentEquals(identity.signingKeyPair.publicKey)
            ) {
                throw RegistrationTransportException("registration request uses different installation keys")
            }
            channelFactory().use { channel ->
                val serverKeys = handshake(channel, identity)
                sendRegistration(channel, request, identity, serverKeys)
            }
        } catch (exception: RegistrationTransportException) {
            throw exception
        } catch (exception: Exception) {
            throw RegistrationTransportException(REGISTRATION_FAILED_MESSAGE, exception)
        }

    private fun handshake(
        channel: FrameChannel,
        identity: InstallationIdentity,
    ): DedicatedServerKeys {
        val bootstrap = FrameBodyCodec.decodeBootstrap(channel.read(), maxFrameBytes)
        val observed =
            DedicatedServerKeys(
                x25519PublicKey = bootstrap.x25519PublicKey,
                ed25519PublicKey = bootstrap.ed25519PublicKey,
            )
        val pinned = identity.dedicatedServerKeys
        if (pinned == null) {
            identityStore.save(identity.copy(dedicatedServerKeys = observed))
            return observed
        }
        if (pinned != observed) {
            throw RegistrationTransportException(SERVER_KEY_MISMATCH_MESSAGE)
        }
        return pinned
    }

    private fun sendRegistration(
        channel: FrameChannel,
        request: RegistrationRequest,
        identity: InstallationIdentity,
        serverKeys: DedicatedServerKeys,
    ): RegistrationResponse {
        val logicalHeader =
            ClientLogicalHeaderCodec.encode(
                ClientLogicalHeader(
                    operation = OperationType.REGISTRATION,
                    login = request.login,
                    password = request.password,
                    arguments =
                        listOf(
                            CborValue.Bytes(identity.agreementKeyPair.publicKey),
                            CborValue.Bytes(identity.signingKeyPair.publicKey),
                        ),
                ),
            )
        val frame =
            FrameBodyCodec.encode(
                header =
                    ExternalFrameHeader(
                        senderId = null,
                        requestId = request.requestId,
                        frameNumber = 0,
                        uploadId = null,
                        responderLabel = null,
                        bootstrapSigningPublicKey = identity.signingKeyPair.publicKey,
                    ),
                payload = FrameBodyCodec.encryptedPayload(serverKeys.x25519PublicKey, logicalHeader),
                sign = { bytes -> Ed25519.sign(identity.signingKeyPair.privateKey, bytes) },
                maxFrameBytes = maxFrameBytes,
            )
        channel.write(frame)

        val response =
            FrameBodyCodec.decode(channel.read(), maxFrameBytes) { bytes, signature ->
                Ed25519.verify(serverKeys.ed25519PublicKey, bytes, signature)
            }
        val envelope =
            ResultEnvelopeCodec.decode(
                FrameBodyCodec.decryptedPayload(identity.agreementKeyPair.privateKey, response.payload),
            )
        return registrationResponse(envelope)
    }

    private fun registrationResponse(envelope: ResultEnvelope): RegistrationResponse {
        if (!envelope.isSuccess) {
            return RegistrationResponse(null)
        }
        val installationId = (envelope.arguments.singleOrNull() as? CborValue.Bytes)?.value
        return if (installationId != null && installationId.isUuidVersion4()) {
            RegistrationResponse(installationId)
        } else {
            RegistrationResponse(null)
        }
    }
}

private fun ByteArray.isUuidVersion4(): Boolean =
    size == INSTALLATION_ID_BYTES &&
        (this[UUID_VERSION_INDEX].toInt() and UUID_VERSION_MASK) == UUID_VERSION_4 &&
        (this[UUID_VARIANT_INDEX].toInt() and UUID_VARIANT_MASK) == UUID_VARIANT_RFC_4122

private fun InstallationIdentity.hasKeyMaterial(): Boolean =
    signingKeyPair.privateKey.isNotEmpty() &&
        signingKeyPair.publicKey.isNotEmpty() &&
        agreementKeyPair.privateKey.isNotEmpty() &&
        agreementKeyPair.publicKey.isNotEmpty()

private const val INSTALLATION_ID_BYTES = 16
private const val UUID_VERSION_INDEX = 6
private const val UUID_VERSION_MASK = 0xF0
private const val UUID_VERSION_4 = 0x40
private const val UUID_VARIANT_INDEX = 8
private const val UUID_VARIANT_MASK = 0xC0
private const val UUID_VARIANT_RFC_4122 = 0x80
private const val IDENTITY_UNAVAILABLE_MESSAGE = "the installation identity is unavailable"
private const val SERVER_KEY_MISMATCH_MESSAGE = "the dedicated server keys do not match the pinned keys"
private const val REGISTRATION_FAILED_MESSAGE = "the registration request failed"
