package dev.homecloud.android.crypto

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
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
    fun keyPairsWithEqualBytesAreEqualAndHashEqual() {
        val keyPair = X25519.generate()
        val identicalKeyPair =
            X25519KeyPair(
                privateKey = keyPair.privateKey.copyOf(),
                publicKey = keyPair.publicKey.copyOf(),
            )

        assertEquals(keyPair, identicalKeyPair)
        assertEquals(keyPair.hashCode(), identicalKeyPair.hashCode())
    }

    @Test
    fun keyPairsWithDifferentPublicKeysAreUnequal() {
        val keyPair = X25519.generate()
        val differentPublicKey = keyPair.publicKey.copyOf()
        differentPublicKey[0] = (differentPublicKey[0].toInt() xor CHANGED_BYTE_MASK).toByte()

        assertNotEquals(
            keyPair,
            X25519KeyPair(privateKey = keyPair.privateKey.copyOf(), publicKey = differentPublicKey),
        )
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

private const val CHANGED_BYTE_MASK = 0x01
private const val KEY_BYTES = 32
