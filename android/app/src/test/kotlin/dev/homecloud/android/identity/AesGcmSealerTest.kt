package dev.homecloud.android.identity

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * Pins the envelope produced by [AesGcmSealer] for `FR-AND-026 / 2`: a payload is the 12-byte IV
 * followed by the AES-256-GCM ciphertext and its 16-byte tag, opening reverses sealing byte for
 * byte, every seal uses a fresh random IV, and tampered, truncated or wrong-key payloads fail as
 * [IdentityStorageException] instead of returning data.
 *
 * The sealer takes a caller-supplied key, so this test runs on the plain JVM without the Android
 * KeyStore provider that Robolectric does not ship.
 */
class AesGcmSealerTest {
    private val key = aesKey()

    @Test
    fun `seal then open returns the original plaintext`() {
        val plaintext = PLAINTEXT.toByteArray(Charsets.UTF_8)

        val opened = AesGcmSealer.open(key, AesGcmSealer.seal(key, plaintext))

        assertArrayEquals(plaintext, opened)
    }

    @Test
    fun `payload is not the plaintext and carries an iv, ciphertext and tag`() {
        val plaintext = PLAINTEXT.toByteArray(Charsets.UTF_8)

        val payload = AesGcmSealer.seal(key, plaintext)

        assertFalse(payload.contentEquals(plaintext))
        assertEquals(IV_BYTES + plaintext.size + TAG_BYTES, payload.size)
    }

    @Test
    fun `two seals of the same plaintext differ`() {
        val plaintext = PLAINTEXT.toByteArray(Charsets.UTF_8)

        val first = AesGcmSealer.seal(key, plaintext)
        val second = AesGcmSealer.seal(key, plaintext)

        assertFalse(first.contentEquals(second))
    }

    @Test
    fun `tampered ciphertext throws`() {
        val payload = AesGcmSealer.seal(key, PLAINTEXT.toByteArray(Charsets.UTF_8))
        val tampered = payload.copyOf()
        tampered[IV_BYTES] = (tampered[IV_BYTES].toInt() xor TAMPER_MASK).toByte()

        assertThrows(IdentityStorageException::class.java) { AesGcmSealer.open(key, tampered) }
    }

    @Test
    fun `wrong key throws`() {
        val payload = AesGcmSealer.seal(key, PLAINTEXT.toByteArray(Charsets.UTF_8))

        assertThrows(IdentityStorageException::class.java) { AesGcmSealer.open(aesKey(), payload) }
    }

    @Test
    fun `payload one byte shorter than the iv and tag throws`() {
        val tooShort = ByteArray(IV_BYTES + TAG_BYTES - 1)

        assertThrows(IdentityStorageException::class.java) { AesGcmSealer.open(key, tooShort) }
    }

    @Test
    fun `empty payload throws`() {
        assertThrows(IdentityStorageException::class.java) { AesGcmSealer.open(key, ByteArray(0)) }
    }

    @Test
    fun `empty plaintext seals and opens back to empty`() {
        val payload = AesGcmSealer.seal(key, ByteArray(0))

        val opened = AesGcmSealer.open(key, payload)

        assertArrayEquals(ByteArray(0), opened)
    }

    private fun aesKey(): SecretKey =
        KeyGenerator
            .getInstance(AES_ALGORITHM)
            .apply { init(KEY_BITS) }
            .generateKey()

    private companion object {
        const val PLAINTEXT = "signingPrivate=test-value"
        const val IV_BYTES = 12
        const val TAG_BYTES = 16

        const val AES_ALGORITHM = "AES"
        const val KEY_BITS = 256
        const val TAMPER_MASK = 0x01
    }
}
