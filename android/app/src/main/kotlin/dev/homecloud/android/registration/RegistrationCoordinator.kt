package dev.homecloud.android.registration

import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import java.nio.ByteBuffer
import java.util.UUID

/** The result of one primary-registration attempt (`FR-AND-028 / 2`, `FR-AND-053 / 1`). */
internal sealed interface RegistrationOutcome {
    data class Registered(
        val installationId: ByteArray,
    ) : RegistrationOutcome

    data object Pending : RegistrationOutcome

    data object StorageFailed : RegistrationOutcome

    data object NoIdentity : RegistrationOutcome

    /** The identity has no pinned dedicated-server keys yet, so registration cannot be encrypted. */
    data object MissingServerKeys : RegistrationOutcome
}

/**
 * Drives primary installation registration with a durable attempt that survives restarts
 * (`FR-AND-053 / 1`, `FR-AND-054 / 1`, `FR-AND-055 / 1`).
 *
 * The attempt is persisted before the first network call, and a retry reuses its stored request id,
 * arguments and public keys. Consequently an interrupted registration is never turned into a new
 * installation while a recoverable pending record exists (`FR-AND-028 / 2`). A record for different
 * keys or a different login cannot apply to the current identity and is discarded.
 *
 * Once the identity carries a sender id, any pending record is stale and is cleared before the
 * assigned id is reported. The same rule recovers a crash between the identity save and the pending
 * clear: the next call clears the pending record and reports the already registered identity
 * (`FR-AND-055 / 1`). Recording the assigned id keeps it in app-private installation storage with
 * the existing key pairs (`FR-AND-029 / 1`). A transport failure or a failed identity save keeps the
 * pending record so the next call retries the same request.
 *
 * Reading, writing and transport are blocking and callers run them off the main thread. Registration
 * needs the dedicated-server keys pinned at first contact (`INT-NET-025 / 1`, `INT-NET-034 / 1`),
 * because the request payload is encrypted to that X25519 key; without them the coordinator reports
 * [RegistrationOutcome.MissingServerKeys] and sends nothing. The password
 * and key material are never logged or placed into an exception message (`FR-AND-023 / 2`).
 */
internal class RegistrationCoordinator(
    private val identityStore: InstallationIdentityStore,
    private val pendingStore: PendingRegistrationStore,
    private val transport: RegistrationTransport,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    fun register(
        login: String,
        password: String,
    ): RegistrationOutcome {
        val identity = identityStore.load() ?: return RegistrationOutcome.NoIdentity
        val assignedSenderId = identity.senderId
        if (assignedSenderId != null) {
            if (pendingStore.load() != null) {
                pendingStore.clear()
            }
            return RegistrationOutcome.Registered(assignedSenderId)
        }
        if (identity.dedicatedServerKeys == null) {
            // The dedicated server's keys are pinned on first contact before registration (INT-NET-025/034).
            return RegistrationOutcome.MissingServerKeys
        }

        val record = pendingRecord(identity, login, password)
        if (!persist(record)) {
            return RegistrationOutcome.StorageFailed
        }

        val response = send(record) ?: return RegistrationOutcome.Pending
        val installationId = response.installationId ?: return RegistrationOutcome.Pending
        if (installationId.size != INSTALLATION_ID_BYTES) {
            return RegistrationOutcome.Pending
        }
        if (!saveSenderId(identity, installationId)) {
            return RegistrationOutcome.Pending
        }

        pendingStore.clear()
        return RegistrationOutcome.Registered(installationId)
    }

    private fun pendingRecord(
        identity: InstallationIdentity,
        login: String,
        password: String,
    ): PendingRegistration {
        val existing = pendingStore.load()
        if (existing != null && existing.matches(identity, login)) {
            return existing
        }
        if (existing != null) {
            pendingStore.clear()
        }
        val x25519PublicKey = identity.agreementKeyPair.publicKey
        val ed25519PublicKey = identity.signingKeyPair.publicKey
        return PendingRegistration(
            requestId = newRequestId(),
            login = login,
            password = password,
            x25519PublicKey = x25519PublicKey,
            ed25519PublicKey = ed25519PublicKey,
            arguments = RegistrationArguments.encode(x25519PublicKey, ed25519PublicKey),
            startedAtMillis = clock(),
        )
    }

    private fun PendingRegistration.matches(
        identity: InstallationIdentity,
        login: String,
    ): Boolean =
        this.login == login &&
            x25519PublicKey.contentEquals(identity.agreementKeyPair.publicKey) &&
            ed25519PublicKey.contentEquals(identity.signingKeyPair.publicKey)

    private fun persist(record: PendingRegistration): Boolean =
        try {
            pendingStore.save(record)
            true
        } catch (_: Exception) {
            false
        }

    private fun send(record: PendingRegistration): RegistrationResponse? =
        try {
            transport.register(
                RegistrationRequest(
                    requestId = record.requestId,
                    login = record.login,
                    password = record.password,
                    x25519PublicKey = record.x25519PublicKey,
                    ed25519PublicKey = record.ed25519PublicKey,
                    arguments = record.arguments,
                ),
            )
        } catch (_: RegistrationTransportException) {
            null
        } catch (_: Exception) {
            null
        }

    private fun saveSenderId(
        identity: InstallationIdentity,
        senderId: ByteArray,
    ): Boolean =
        try {
            identityStore.save(identity.copy(senderId = senderId))
            true
        } catch (_: Exception) {
            false
        }

    private fun newRequestId(): ByteArray {
        val uuid = UUID.randomUUID()
        return ByteBuffer
            .allocate(REQUEST_ID_BYTES)
            .putLong(uuid.mostSignificantBits)
            .putLong(uuid.leastSignificantBits)
            .array()
    }

    private companion object {
        const val REQUEST_ID_BYTES = 16
        const val INSTALLATION_ID_BYTES = 16
    }
}
