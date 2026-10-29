package dev.homecloud.android.crypto

import dev.homecloud.android.testsupport.Hex
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class Ed25519Test {
    @Test
    fun rfc8032VectorsDerivePublicKeys() {
        assertArrayEquals(Hex.decode(RFC8032_TEST_1_PUBLIC_KEY), Ed25519.publicKeyOf(Hex.decode(RFC8032_TEST_1_SEED)))
        assertArrayEquals(Hex.decode(RFC8032_TEST_2_PUBLIC_KEY), Ed25519.publicKeyOf(Hex.decode(RFC8032_TEST_2_SEED)))
        assertArrayEquals(Hex.decode(RFC8032_TEST_3_PUBLIC_KEY), Ed25519.publicKeyOf(Hex.decode(RFC8032_TEST_3_SEED)))
    }

    @Test
    fun rfc8032VectorsProduceSignatures() {
        assertArrayEquals(
            Hex.decode(RFC8032_TEST_1_SIGNATURE),
            Ed25519.sign(Hex.decode(RFC8032_TEST_1_SEED), ByteArray(0)),
        )
        assertArrayEquals(
            Hex.decode(RFC8032_TEST_2_SIGNATURE),
            Ed25519.sign(Hex.decode(RFC8032_TEST_2_SEED), Hex.decode(RFC8032_TEST_2_MESSAGE)),
        )
        assertArrayEquals(
            Hex.decode(RFC8032_TEST_3_SIGNATURE),
            Ed25519.sign(Hex.decode(RFC8032_TEST_3_SEED), Hex.decode(RFC8032_TEST_3_MESSAGE)),
        )
    }

    @Test
    fun rfc8032VectorsVerify() {
        assertTrue(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_1_PUBLIC_KEY),
                ByteArray(0),
                Hex.decode(RFC8032_TEST_1_SIGNATURE),
            ),
        )
        assertTrue(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_2_PUBLIC_KEY),
                Hex.decode(RFC8032_TEST_2_MESSAGE),
                Hex.decode(RFC8032_TEST_2_SIGNATURE),
            ),
        )
        assertTrue(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_3_PUBLIC_KEY),
                Hex.decode(RFC8032_TEST_3_MESSAGE),
                Hex.decode(RFC8032_TEST_3_SIGNATURE),
            ),
        )
    }

    @Test
    fun verifyRejectsTamperedSignature() {
        val tamperedSignature =
            Hex.decode(RFC8032_TEST_3_SIGNATURE).also { signature ->
                signature[0] = (signature[0].toInt() xor TAMPER_MASK).toByte()
            }

        assertFalse(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_3_PUBLIC_KEY),
                Hex.decode(RFC8032_TEST_3_MESSAGE),
                tamperedSignature,
            ),
        )
    }

    @Test
    fun verifyRejectsTamperedMessage() {
        val tamperedMessage =
            Hex.decode(RFC8032_TEST_2_MESSAGE).also { message ->
                message[0] = (message[0].toInt() xor TAMPER_MASK).toByte()
            }

        assertFalse(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_2_PUBLIC_KEY),
                tamperedMessage,
                Hex.decode(RFC8032_TEST_2_SIGNATURE),
            ),
        )
    }

    @Test
    fun verifyRejectsShortPublicKey() {
        assertFalse(
            Ed25519.verify(
                ByteArray(PUBLIC_KEY_BYTES - 1),
                ByteArray(0),
                Hex.decode(RFC8032_TEST_1_SIGNATURE),
            ),
        )
    }

    @Test
    fun verifyRejectsShortSignature() {
        assertFalse(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_1_PUBLIC_KEY),
                ByteArray(0),
                ByteArray(SIGNATURE_BYTES - 1),
            ),
        )
    }

    @Test
    fun verifyRejectsOversizedSignature() {
        assertFalse(
            Ed25519.verify(
                Hex.decode(RFC8032_TEST_1_PUBLIC_KEY),
                ByteArray(0),
                ByteArray(SIGNATURE_BYTES + 1),
            ),
        )
    }

    @Test
    fun signRejectsWrongLengthPrivateKey() {
        for (size in WRONG_PRIVATE_KEY_BYTES) {
            assertThrows(CryptoException::class.java) {
                Ed25519.sign(ByteArray(size), ByteArray(0))
            }
        }
    }

    @Test
    fun publicKeyOfRejectsWrongLengthPrivateKey() {
        for (size in WRONG_PRIVATE_KEY_BYTES) {
            assertThrows(CryptoException::class.java) {
                Ed25519.publicKeyOf(ByteArray(size))
            }
        }
    }

    @Test
    fun generatedKeyPairRoundTripsSignature() {
        val keyPair = Ed25519.generate()
        val message = "interoperability".toByteArray(Charsets.UTF_8)
        val signature = Ed25519.sign(keyPair.privateKey, message)

        assertEquals(PRIVATE_KEY_BYTES, keyPair.privateKey.size)
        assertEquals(PUBLIC_KEY_BYTES, keyPair.publicKey.size)
        assertEquals(SIGNATURE_BYTES, signature.size)
        assertTrue(Ed25519.verify(keyPair.publicKey, message, signature))
    }
}

private const val TAMPER_MASK = 0x01
private const val PRIVATE_KEY_BYTES = 32
private const val PUBLIC_KEY_BYTES = 32
private const val SIGNATURE_BYTES = 64
private val WRONG_PRIVATE_KEY_BYTES = intArrayOf(PRIVATE_KEY_BYTES - 1, PRIVATE_KEY_BYTES + 1)

private const val RFC8032_TEST_1_SEED =
    "9d61b19deffd5a60ba844af492ec2cc4" +
        "4449c5697b326919703bac031cae7f60"
private const val RFC8032_TEST_1_PUBLIC_KEY =
    "d75a980182b10ab7d54bfed3c964073a" +
        "0ee172f3daa62325af021a68f707511a"
private const val RFC8032_TEST_1_SIGNATURE =
    "e5564300c360ac729086e2cc806e828a" +
        "84877f1eb8e5d974d873e06522490155" +
        "5fb8821590a33bacc61e39701cf9b46b" +
        "d25bf5f0595bbe24655141438e7a100b"

private const val RFC8032_TEST_2_SEED =
    "4ccd089b28ff96da9db6c346ec114e0f" +
        "5b8a319f35aba624da8cf6ed4fb8a6fb"
private const val RFC8032_TEST_2_PUBLIC_KEY =
    "3d4017c3e843895a92b70aa74d1b7ebc" +
        "9c982ccf2ec4968cc0cd55f12af4660c"
private const val RFC8032_TEST_2_MESSAGE = "72"
private const val RFC8032_TEST_2_SIGNATURE =
    "92a009a9f0d4cab8720e820b5f642540" +
        "a2b27b5416503f8fb3762223ebdb69da" +
        "085ac1e43e15996e458f3613d0f11d8c" +
        "387b2eaeb4302aeeb00d291612bb0c00"

private const val RFC8032_TEST_3_SEED =
    "c5aa8df43f9f837bedb7442f31dcb7b1" +
        "66d38535076f094b85ce3a2e0b4458f7"
private const val RFC8032_TEST_3_PUBLIC_KEY =
    "fc51cd8e6218a1a38da47ed00230f058" +
        "0816ed13ba3303ac5deb911548908025"
private const val RFC8032_TEST_3_MESSAGE = "af82"
private const val RFC8032_TEST_3_SIGNATURE =
    "6291d657deec24024827e69c3abe01a3" +
        "0ce548a284743a445e3680d7db5ac3ac" +
        "18ff9b538d16f290ae67f760984dc659" +
        "4a7c15e9716ed28dc027beceea1ec40a"
