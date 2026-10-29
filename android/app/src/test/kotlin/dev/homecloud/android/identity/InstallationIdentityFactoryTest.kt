package dev.homecloud.android.identity

import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519KeyPair
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Asserts the identity-generation rules of `FR-DED-022 / 1` and `NFR-SEC-003 / 1`: a new installation
 * gets a fresh 32-byte Ed25519 pair and a separate 32-byte X25519 pair, and the registration-derived
 * values start absent. It also asserts that identities and their nested key material compare by
 * value.
 */
class InstallationIdentityFactoryTest {
    @Test
    fun `new identity has 32-byte signing and agreement keys`() {
        val identity = InstallationIdentityFactory.newIdentity()

        assertEquals(KEY_BYTES, identity.signingKeyPair.privateKey.size)
        assertEquals(KEY_BYTES, identity.signingKeyPair.publicKey.size)
        assertEquals(KEY_BYTES, identity.agreementKeyPair.privateKey.size)
        assertEquals(KEY_BYTES, identity.agreementKeyPair.publicKey.size)
    }

    @Test
    fun `signing and agreement key material differs`() {
        val identity = InstallationIdentityFactory.newIdentity()

        assertFalse(identity.signingKeyPair.privateKey.contentEquals(identity.agreementKeyPair.privateKey))
        assertFalse(identity.signingKeyPair.publicKey.contentEquals(identity.agreementKeyPair.publicKey))
    }

    @Test
    fun `new identity has no sender id and no dedicated server keys`() {
        val identity = InstallationIdentityFactory.newIdentity()

        assertNull(identity.senderId)
        assertNull(identity.dedicatedServerKeys)
    }

    @Test
    fun `two new identities have different private keys`() {
        val first = InstallationIdentityFactory.newIdentity()
        val second = InstallationIdentityFactory.newIdentity()

        assertFalse(first.signingKeyPair.privateKey.contentEquals(second.signingKeyPair.privateKey))
        assertFalse(first.agreementKeyPair.privateKey.contentEquals(second.agreementKeyPair.privateKey))
    }

    @Test
    fun `identities with equal content are equal and hash equal`() {
        val first = identity(senderId = keyBytes(SENDER_ID_SEED), dedicatedServerKeys = serverKeys())
        val second = identity(senderId = keyBytes(SENDER_ID_SEED), dedicatedServerKeys = serverKeys())

        assertEquals(first, second)
        assertEquals(first.hashCode(), second.hashCode())
    }

    @Test
    fun `identities with different sender ids are unequal`() {
        assertNotEquals(
            identity(senderId = keyBytes(SENDER_ID_SEED)),
            identity(senderId = keyBytes(SENDER_ID_SEED + 1)),
        )
    }

    @Test
    fun `identity without dedicated server keys differs from one with them`() {
        assertNotEquals(identity(), identity(dedicatedServerKeys = serverKeys()))
    }

    @Test
    fun `dedicated server keys with equal arrays are equal and hash equal`() {
        val first = serverKeys()
        val second = serverKeys()

        assertEquals(first, second)
        assertEquals(first.hashCode(), second.hashCode())
    }

    @Test
    fun `dedicated server keys with different arrays are unequal`() {
        assertNotEquals(
            DedicatedServerKeys(
                x25519PublicKey = keyBytes(SERVER_KEY_SEED),
                ed25519PublicKey = keyBytes(SERVER_KEY_SEED + 1),
            ),
            DedicatedServerKeys(
                x25519PublicKey = keyBytes(SERVER_KEY_SEED + 2),
                ed25519PublicKey = keyBytes(SERVER_KEY_SEED + 1),
            ),
        )
    }
}

private fun identity(
    senderId: ByteArray? = null,
    dedicatedServerKeys: DedicatedServerKeys? = null,
): InstallationIdentity =
    InstallationIdentity(
        signingKeyPair = Ed25519KeyPair(privateKey = keyBytes(SIGNING_KEY_SEED), publicKey = keyBytes(SIGNING_KEY_SEED + 1)),
        agreementKeyPair = X25519KeyPair(privateKey = keyBytes(AGREEMENT_KEY_SEED), publicKey = keyBytes(AGREEMENT_KEY_SEED + 1)),
        senderId = senderId,
        dedicatedServerKeys = dedicatedServerKeys,
    )

private fun serverKeys(): DedicatedServerKeys =
    DedicatedServerKeys(
        x25519PublicKey = keyBytes(SERVER_KEY_SEED),
        ed25519PublicKey = keyBytes(SERVER_KEY_SEED + 1),
    )

private fun keyBytes(seed: Int): ByteArray = ByteArray(KEY_BYTES) { offset -> (offset + seed).toByte() }

private const val AGREEMENT_KEY_SEED = 3
private const val KEY_BYTES = 32
private const val SENDER_ID_SEED = 8
private const val SERVER_KEY_SEED = 5
private const val SIGNING_KEY_SEED = 1
