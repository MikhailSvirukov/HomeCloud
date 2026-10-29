package dev.homecloud.android.identity

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Asserts the identity-generation rules of `FR-DED-022 / 1` and `NFR-SEC-003 / 1`: a new installation
 * gets a fresh 32-byte Ed25519 pair and a separate 32-byte X25519 pair, and the registration-derived
 * values start absent.
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
}

private const val KEY_BYTES = 32
