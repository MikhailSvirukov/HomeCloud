package dev.homecloud.android.registration

import dev.homecloud.android.identity.InstallationIdentityFactory
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.ClientNetworkConfiguration
import dev.homecloud.android.net.FrameChannel

/**
 * Drives primary installation registration for the setup screen (`FR-AND-028 / 2`,
 * `FR-AND-029 / 1`, `FR-AND-053 / 1`, `FR-AND-054 / 1`, `FR-AND-055 / 1`).
 *
 * A missing installation identity is created and stored before any transport is opened, and an
 * existing identity is reused unchanged, so repeated attempts never replace stored key material. A
 * failed identity save ends the attempt before the first network operation. Once the identity
 * exists, one durable registration attempt runs through [RegistrationCoordinator], which persists it
 * before the first network send and reuses the stored request across restarts.
 *
 * Reading, writing and the network are blocking, so callers run registration off the main thread.
 * The password and key material are never logged or placed into an exception message
 * (`FR-AND-023 / 2`).
 */
internal class RegistrationController(
    private val identityStore: InstallationIdentityStore,
    private val pendingStore: PendingRegistrationStore,
    private val channelFactory: (host: String, port: Int) -> FrameChannel,
    private val maxFrameBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    /**
     * Creates the installation identity if it does not exist yet, then runs one durable
     * registration attempt (`FR-AND-028 / 2`, `FR-AND-029 / 1`, `FR-AND-053 / 1`,
     * `FR-AND-054 / 1`, `FR-AND-055 / 1`).
     */
    fun register(
        host: String,
        port: Int,
        login: String,
        password: String,
    ): RegistrationOutcome {
        if (identityStore.load() == null && !createIdentity()) {
            return RegistrationOutcome.StorageFailed
        }
        val transport =
            DedicatedServerTransport(
                identityStore = identityStore,
                channelFactory = { channelFactory(host, port) },
                maxFrameBytes = maxFrameBytes,
            )
        return RegistrationCoordinator(identityStore, pendingStore, transport, clock).register(login, password)
    }

    private fun createIdentity(): Boolean =
        try {
            identityStore.save(InstallationIdentityFactory.newIdentity())
            true
        } catch (_: Exception) {
            false
        }
}
