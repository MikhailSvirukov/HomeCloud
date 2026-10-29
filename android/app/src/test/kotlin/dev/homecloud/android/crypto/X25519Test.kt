package dev.homecloud.android.crypto

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class X25519Test {
    @Test
    fun generatedKeyPairsAgreeOnSameSecretInBothDirections() {
        val first = X25519.generate()
        val second = X25519.generate()

        val firstSecret = X25519.sharedSecret(first.privateKey, second.publicKey)
        val secondSecret = X25519.sharedSecret(second.privateKey, first.publicKey)

        assertArrayEquals(firstSecret, secondSecret)
        assertEquals(KEY_BYTES, firstSecret.size)
    }

    @Test
    fun publicKeyOfMatchesGeneratedPublicKey() {
        val keyPair = X25519.generate()

        assertArrayEquals(keyPair.publicKey, X25519.publicKeyOf(keyPair.privateKey))
    }

    @Test
    fun sharedSecretRejectsWrongLengthPrivateKey() {
        val peer = X25519.generate()

        assertThrows(CryptoException::class.java) {
            X25519.sharedSecret(ByteArray(KEY_BYTES - 1), peer.publicKey)
        }
    }

    @Test
    fun sharedSecretRejectsWrongLengthPeerPublicKey() {
        val keyPair = X25519.generate()

        assertThrows(CryptoException::class.java) {
            X25519.sharedSecret(keyPair.privateKey, ByteArray(KEY_BYTES + 1))
        }
    }

    @Test
    fun sharedSecretRejectsAllZeroPeerPublicKey() {
        val keyPair = X25519.generate()

        assertThrows(CryptoException::class.java) {
            X25519.sharedSecret(keyPair.privateKey, ByteArray(KEY_BYTES))
        }
    }
}

private const val KEY_BYTES = 32
