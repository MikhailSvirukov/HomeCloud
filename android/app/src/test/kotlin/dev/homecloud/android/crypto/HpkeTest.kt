package dev.homecloud.android.crypto

import dev.homecloud.android.testsupport.Hex
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class HpkeTest {
    @Test
    fun rfc9180A11SealProducesVectorCiphertext() {
        val sealed = sealRfcVector()

        assertArrayEquals(Hex.decode(RFC_ENCAPSULATED_KEY), sealed.encapsulatedKey)
        assertArrayEquals(Hex.decode(RFC_CIPHERTEXT), sealed.ciphertext)
    }

    @Test
    fun rfc9180A11OpenRecoversVectorPlaintext() {
        val plaintext =
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = Hex.decode(RFC_ENCAPSULATED_KEY),
                aad = Hex.decode(RFC_AAD),
                ciphertext = Hex.decode(RFC_CIPHERTEXT),
                info = Hex.decode(RFC_INFO),
            )

        assertArrayEquals(Hex.decode(RFC_PLAINTEXT), plaintext)
    }

    @Test
    fun openRejectsTamperedCiphertext() {
        val sealed = sealRfcVector()
        val tamperedCiphertext =
            sealed.ciphertext.copyOf().also { ciphertext ->
                ciphertext[0] = (ciphertext[0].toInt() xor TAMPER_MASK).toByte()
            }

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(RFC_AAD),
                ciphertext = tamperedCiphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsWrongAad() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(WRONG_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsWrongRecipientPrivateKey() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(WRONG_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun defaultAeadRoundTripReturnsPlaintext() {
        val recipient = X25519.generate()
        val plaintext = "photo payload".toByteArray(Charsets.UTF_8)
        val sealed =
            Hpke.seal(
                recipientPublicKey = recipient.publicKey,
                aad = DEFAULT_AAD,
                plaintext = plaintext,
            )

        assertEquals(X25519_KEY_BYTES, sealed.encapsulatedKey.size)
        val opened =
            Hpke.open(
                recipientPrivateKey = recipient.privateKey,
                encapsulatedKey = sealed.encapsulatedKey,
                aad = DEFAULT_AAD,
                ciphertext = sealed.ciphertext,
            )

        assertArrayEquals(plaintext, opened)
    }

    @Test
    fun sealRejectsWrongLengthRecipientPublicKey() {
        assertThrows(CryptoException::class.java) {
            Hpke.seal(
                recipientPublicKey = ByteArray(X25519_KEY_BYTES - 1),
                aad = DEFAULT_AAD,
                plaintext = byteArrayOf(1),
            )
        }
    }

    @Test
    fun sealRejectsOversizedRecipientPublicKey() {
        assertThrows(CryptoException::class.java) {
            Hpke.seal(
                recipientPublicKey = ByteArray(X25519_KEY_BYTES + 1),
                aad = DEFAULT_AAD,
                plaintext = byteArrayOf(1),
            )
        }
    }

    @Test
    fun sealRejectsWrongLengthEphemeralPrivateKey() {
        val recipient = X25519.generate()

        assertThrows(CryptoException::class.java) {
            Hpke.seal(
                recipientPublicKey = recipient.publicKey,
                ephemeralPrivateKey = ByteArray(X25519_KEY_BYTES - 1),
                aad = DEFAULT_AAD,
                plaintext = byteArrayOf(1),
            )
        }
    }

    @Test
    fun sealRejectsOversizedEphemeralPrivateKey() {
        val recipient = X25519.generate()

        assertThrows(CryptoException::class.java) {
            Hpke.seal(
                recipientPublicKey = recipient.publicKey,
                ephemeralPrivateKey = ByteArray(X25519_KEY_BYTES + 1),
                aad = DEFAULT_AAD,
                plaintext = byteArrayOf(1),
            )
        }
    }

    @Test
    fun openRejectsWrongLengthRecipientPrivateKey() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = ByteArray(X25519_KEY_BYTES - 1),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsWrongLengthEncapsulatedKey() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = ByteArray(X25519_KEY_BYTES - 1),
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsOversizedRecipientPrivateKey() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = ByteArray(X25519_KEY_BYTES + 1),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsOversizedEncapsulatedKey() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = ByteArray(X25519_KEY_BYTES + 1),
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext,
                info = Hex.decode(RFC_INFO),
            )
        }
    }

    @Test
    fun openRejectsCiphertextShorterThanAuthenticationTag() {
        val sealed = sealRfcVector()

        assertThrows(CryptoException::class.java) {
            Hpke.open(
                aead = HpkeAead.AES_128_GCM,
                recipientPrivateKey = Hex.decode(RFC_RECIPIENT_PRIVATE_KEY),
                encapsulatedKey = sealed.encapsulatedKey,
                aad = Hex.decode(RFC_AAD),
                ciphertext = sealed.ciphertext.copyOf(AUTHENTICATION_TAG_BYTES - 1),
                info = Hex.decode(RFC_INFO),
            )
        }
    }
}

private fun sealRfcVector(): HpkeCiphertext =
    Hpke.seal(
        aead = HpkeAead.AES_128_GCM,
        recipientPublicKey = Hex.decode(RFC_RECIPIENT_PUBLIC_KEY),
        ephemeralPrivateKey = Hex.decode(RFC_EPHEMERAL_PRIVATE_KEY),
        aad = Hex.decode(RFC_AAD),
        plaintext = Hex.decode(RFC_PLAINTEXT),
        info = Hex.decode(RFC_INFO),
    )

private const val TAMPER_MASK = 0x01
private const val X25519_KEY_BYTES = 32
private const val AUTHENTICATION_TAG_BYTES = 16
private val DEFAULT_AAD = "Count-0".toByteArray(Charsets.UTF_8)

private const val RFC_INFO = "4f6465206f6e2061204772656369616e2055726e"
private const val RFC_RECIPIENT_PRIVATE_KEY =
    "4612c550263fc8ad58375df3f557aac5" +
        "31d26850903e55a9f23f21d8534e8ac8"
private const val RFC_RECIPIENT_PUBLIC_KEY =
    "3948cfe0ad1ddb695d780e59077195da" +
        "6c56506b027329794ab02bca80815c4d"
private const val RFC_EPHEMERAL_PRIVATE_KEY =
    "52c4a758a802cd8b936eceea31443279" +
        "8d5baf2d7e9235dc084ab1b9cfa2f736"
private const val RFC_ENCAPSULATED_KEY =
    "37fda3567bdbd628e88668c3c8d7e97d" +
        "1d1253b6d4ea6d44c150f741f1bf4431"
private const val RFC_AAD = "436f756e742d30"
private const val WRONG_AAD = "436f756e742d31"
private const val RFC_PLAINTEXT = "4265617574792069732074727574682c20747275746820626561757479"
private const val RFC_CIPHERTEXT =
    "f938558b5d72f1a23810b4be2ab4f843" +
        "31acc02fc97babc53a52ae8218a355a9" +
        "6d8770ac83d07bea87e13c512a"
private const val WRONG_RECIPIENT_PRIVATE_KEY =
    "77076d0a7318a57d3c16c17251b26645" +
        "df4c2f87ebc0992ab177fba51db92c2a"
