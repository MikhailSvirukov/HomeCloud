package dev.homecloud.android.identity

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.crypto.X25519KeyPair

/**
 * The open keys of the dedicated server as first observed by this installation (`INT-NET-025 / 1`,
 * `INT-NET-034 / 1`).
 */
internal data class DedicatedServerKeys(
    val x25519PublicKey: ByteArray,
    val ed25519PublicKey: ByteArray,
)

/**
 * The per-installation signing and encryption identity (`FR-DED-022 / 1`, `NFR-SEC-003 / 1`).
 *
 * The Ed25519 signing pair and the X25519 agreement pair are generated separately and are never
 * derived from each other. The sender id is absent until registration assigns it
 * (`INT-NET-008 / 2`), and the dedicated server keys are absent until the first contact pins them
 * (`INT-NET-025 / 1`, `INT-NET-034 / 1`).
 */
internal data class InstallationIdentity(
    val signingKeyPair: Ed25519KeyPair,
    val agreementKeyPair: X25519KeyPair,
    val senderId: ByteArray?,
    val dedicatedServerKeys: DedicatedServerKeys?,
)

/**
 * Persists the installation identity across app restarts (`FR-DED-022 / 1`).
 *
 * Reading and writing are blocking file I/O, so callers run them off the main thread.
 */
internal interface InstallationIdentityStore {
    /**
     * Produces the stored identity, or no identity when none is stored or the stored content does
     * not validate (`FR-DED-022 / 1`, `INT-NET-008 / 2`, `INT-NET-025 / 1`). A malformed store is
     * reported as absence rather than an exception, so a new identity can be generated.
     */
    fun load(): InstallationIdentity?

    /** Replaces the stored identity in full (`FR-DED-022 / 1`, `INT-NET-008 / 2`). */
    fun save(identity: InstallationIdentity)
}

/**
 * Creates the identity of a new installation (`FR-DED-022 / 1`, `NFR-SEC-003 / 1`,
 * `NFR-SEC-006 / 1`).
 *
 * A fresh Ed25519 signing pair and a fresh X25519 agreement pair come from the existing cryptographic
 * primitives. The identity starts without a sender id and without dedicated server keys, so the
 * first connection registers it and pins the server keys (`INT-NET-008 / 2`, `INT-NET-025 / 1`,
 * `INT-NET-034 / 1`).
 */
internal object InstallationIdentityFactory {
    fun newIdentity(): InstallationIdentity =
        InstallationIdentity(
            signingKeyPair = Ed25519.generate(),
            agreementKeyPair = X25519.generate(),
            senderId = null,
            dedicatedServerKeys = null,
        )
}
