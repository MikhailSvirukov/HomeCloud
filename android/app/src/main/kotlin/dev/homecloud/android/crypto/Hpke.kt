package dev.homecloud.android.crypto

import org.bouncycastle.crypto.InvalidCipherTextException
import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.crypto.engines.AESEngine
import org.bouncycastle.crypto.generators.HKDFBytesGenerator
import org.bouncycastle.crypto.modes.GCMBlockCipher
import org.bouncycastle.crypto.params.AEADParameters
import org.bouncycastle.crypto.params.HKDFParameters
import org.bouncycastle.crypto.params.KeyParameter

/**
 * The AEAD identifiers and key sizes defined by RFC 9180 for the v1 HPKE suite (`INT-NET-027`).
 */
internal enum class HpkeAead(
    val id: Int,
    val keyBytes: Int,
) {
    AES_128_GCM(1, 16),
    AES_256_GCM(2, 32),
}

/**
 * A single-shot HPKE ciphertext consisting of the encapsulated ephemeral public key and the sealed
 * payload.
 */
internal class HpkeCiphertext(
    val encapsulatedKey: ByteArray,
    val ciphertext: ByteArray,
)

/**
 * HPKE Base mode sealing and opening (`INT-NET-027`, `INT-NET-028`).
 *
 * The suite is DHKEM(X25519, HKDF-SHA-256), HKDF-SHA-256 and AES-GCM in Base mode. Each call
 * creates one context and seals or opens exactly one message, so the sequence-0 nonce is the base
 * nonce derived by the key schedule. Malformed key material, a low-order peer point and any
 * authentication failure are rejected.
 */
internal object Hpke {
    /** Base mode, DHKEM(X25519, HKDF-SHA-256), HKDF-SHA-256, AES-256-GCM with a fresh ephemeral key. */
    fun seal(
        recipientPublicKey: ByteArray,
        aad: ByteArray,
        plaintext: ByteArray,
        info: ByteArray = ByteArray(0),
    ): HpkeCiphertext =
        seal(
            aead = HpkeAead.AES_256_GCM,
            recipientPublicKey = recipientPublicKey,
            ephemeralPrivateKey = X25519.generate().privateKey,
            aad = aad,
            plaintext = plaintext,
            info = info,
        )

    /**
     * Deterministic variant for interoperability tests. Production must use a fresh ephemeral key for
     * every message: reusing one repeats the AEAD key and nonce.
     */
    internal fun seal(
        recipientPublicKey: ByteArray,
        ephemeralPrivateKey: ByteArray,
        aad: ByteArray,
        plaintext: ByteArray,
        info: ByteArray = ByteArray(0),
    ): HpkeCiphertext =
        seal(
            aead = HpkeAead.AES_256_GCM,
            recipientPublicKey = recipientPublicKey,
            ephemeralPrivateKey = ephemeralPrivateKey,
            aad = aad,
            plaintext = plaintext,
            info = info,
        )

    fun open(
        recipientPrivateKey: ByteArray,
        encapsulatedKey: ByteArray,
        aad: ByteArray,
        ciphertext: ByteArray,
        info: ByteArray = ByteArray(0),
    ): ByteArray =
        open(
            aead = HpkeAead.AES_256_GCM,
            recipientPrivateKey = recipientPrivateKey,
            encapsulatedKey = encapsulatedKey,
            aad = aad,
            ciphertext = ciphertext,
            info = info,
        )

    // Test seam with an explicit AEAD id (the RFC 9180 A.1 vector uses AES-128-GCM).
    internal fun seal(
        aead: HpkeAead,
        recipientPublicKey: ByteArray,
        ephemeralPrivateKey: ByteArray,
        aad: ByteArray,
        plaintext: ByteArray,
        info: ByteArray,
    ): HpkeCiphertext {
        requireLength(recipientPublicKey, X25519_KEY_BYTES, "HPKE recipient public key")
        requireLength(ephemeralPrivateKey, X25519_KEY_BYTES, "HPKE ephemeral private key")
        val encapsulatedKey = X25519.publicKeyOf(ephemeralPrivateKey)
        val dh = X25519.sharedSecret(ephemeralPrivateKey, recipientPublicKey)
        val sharedSecret = kemSharedSecret(dh, encapsulatedKey, recipientPublicKey)
        val scheduledKey = keySchedule(aead, sharedSecret, info)
        val ciphertext = aesGcmSeal(scheduledKey.key, scheduledKey.baseNonce, aad, plaintext)
        return HpkeCiphertext(encapsulatedKey = encapsulatedKey, ciphertext = ciphertext)
    }

    internal fun open(
        aead: HpkeAead,
        recipientPrivateKey: ByteArray,
        encapsulatedKey: ByteArray,
        aad: ByteArray,
        ciphertext: ByteArray,
        info: ByteArray,
    ): ByteArray {
        requireLength(recipientPrivateKey, X25519_KEY_BYTES, "HPKE recipient private key")
        requireLength(encapsulatedKey, X25519_KEY_BYTES, "HPKE encapsulated key")
        if (ciphertext.size < TAG_BYTES) {
            throw CryptoException("HPKE ciphertext must contain a $TAG_BYTES-byte authentication tag")
        }
        val recipientPublicKey = X25519.publicKeyOf(recipientPrivateKey)
        val dh = X25519.sharedSecret(recipientPrivateKey, encapsulatedKey)
        val sharedSecret = kemSharedSecret(dh, encapsulatedKey, recipientPublicKey)
        val scheduledKey = keySchedule(aead, sharedSecret, info)
        return aesGcmOpen(scheduledKey.key, scheduledKey.baseNonce, aad, ciphertext)
    }
}

private class ScheduledKey(
    val key: ByteArray,
    val baseNonce: ByteArray,
)

private fun kemSharedSecret(
    dh: ByteArray,
    encapsulatedKey: ByteArray,
    recipientPublicKey: ByteArray,
): ByteArray {
    val kemContext = concat(encapsulatedKey, recipientPublicKey)
    val prk = labeledExtract(KEM_SUITE_ID, EMPTY, EAE_PRK_LABEL, dh)
    return labeledExpand(KEM_SUITE_ID, prk, SHARED_SECRET_LABEL, kemContext, SHARED_SECRET_BYTES)
}

private fun keySchedule(
    aead: HpkeAead,
    sharedSecret: ByteArray,
    info: ByteArray,
): ScheduledKey {
    val suiteId = hpkeSuiteId(aead)
    val pskIdHash = labeledExtract(suiteId, EMPTY, PSK_ID_HASH_LABEL, EMPTY)
    val infoHash = labeledExtract(suiteId, EMPTY, INFO_HASH_LABEL, info)
    val keyScheduleContext = concat(byteArrayOf(MODE_BASE), pskIdHash, infoHash)
    val secret = labeledExtract(suiteId, sharedSecret, SECRET_LABEL, EMPTY)
    return ScheduledKey(
        key = labeledExpand(suiteId, secret, KEY_LABEL, keyScheduleContext, aead.keyBytes),
        baseNonce = labeledExpand(suiteId, secret, BASE_NONCE_LABEL, keyScheduleContext, NONCE_BYTES),
    )
}

private fun hpkeSuiteId(aead: HpkeAead): ByteArray =
    concat(
        HPKE_SUITE_LABEL.toByteArray(Charsets.US_ASCII),
        twoByteBigEndian(KEM_ID),
        twoByteBigEndian(KDF_ID),
        twoByteBigEndian(aead.id),
    )

private fun labeledExtract(
    suiteId: ByteArray,
    salt: ByteArray,
    label: String,
    ikm: ByteArray,
): ByteArray =
    extract(
        salt = salt,
        ikm = concat(HPKE_VERSION_LABEL.toByteArray(Charsets.US_ASCII), suiteId, label.toByteArray(Charsets.US_ASCII), ikm),
    )

private fun labeledExpand(
    suiteId: ByteArray,
    prk: ByteArray,
    label: String,
    info: ByteArray,
    length: Int,
): ByteArray =
    expand(
        prk = prk,
        info =
            concat(
                twoByteBigEndian(length),
                HPKE_VERSION_LABEL.toByteArray(Charsets.US_ASCII),
                suiteId,
                label.toByteArray(Charsets.US_ASCII),
                info,
            ),
        length = length,
    )

private fun extract(
    salt: ByteArray,
    ikm: ByteArray,
): ByteArray = HKDFBytesGenerator(SHA256Digest()).extractPRK(salt, ikm)

private fun expand(
    prk: ByteArray,
    info: ByteArray,
    length: Int,
): ByteArray {
    val generator = HKDFBytesGenerator(SHA256Digest())
    generator.init(HKDFParameters.skipExtractParameters(prk, info))
    val output = ByteArray(length)
    generator.generateBytes(output, 0, length)
    return output
}

private fun aesGcmSeal(
    key: ByteArray,
    nonce: ByteArray,
    aad: ByteArray,
    plaintext: ByteArray,
): ByteArray {
    val cipher = GCMBlockCipher.newInstance(AESEngine.newInstance())
    cipher.init(true, AEADParameters(KeyParameter(key), TAG_BITS, nonce, aad))
    val output = ByteArray(cipher.getOutputSize(plaintext.size))
    var written = cipher.processBytes(plaintext, 0, plaintext.size, output, 0)
    written += cipher.doFinal(output, written)
    return output.copyOf(written)
}

private fun aesGcmOpen(
    key: ByteArray,
    nonce: ByteArray,
    aad: ByteArray,
    ciphertext: ByteArray,
): ByteArray {
    val cipher = GCMBlockCipher.newInstance(AESEngine.newInstance())
    cipher.init(false, AEADParameters(KeyParameter(key), TAG_BITS, nonce, aad))
    val output = ByteArray(cipher.getOutputSize(ciphertext.size))
    return try {
        var written = cipher.processBytes(ciphertext, 0, ciphertext.size, output, 0)
        written += cipher.doFinal(output, written)
        output.copyOf(written)
    } catch (_: InvalidCipherTextException) {
        throw CryptoException("HPKE authentication failed")
    }
}

private fun requireLength(
    value: ByteArray,
    expected: Int,
    name: String,
) {
    if (value.size != expected) {
        throw CryptoException("$name must be $expected bytes")
    }
}

private fun concat(vararg parts: ByteArray): ByteArray {
    val output = ByteArray(parts.sumOf { part -> part.size })
    var offset = 0
    for (part in parts) {
        part.copyInto(output, offset)
        offset += part.size
    }
    return output
}

private fun twoByteBigEndian(value: Int): ByteArray =
    byteArrayOf(
        (value ushr BITS_PER_BYTE).toByte(),
        value.toByte(),
    )

private const val BITS_PER_BYTE = 8
private const val MODE_BASE: Byte = 0
private const val KEM_ID = 0x0020
private const val KDF_ID = 0x0001
private const val SHARED_SECRET_BYTES = 32
private const val X25519_KEY_BYTES = 32
private const val NONCE_BYTES = 12
private const val TAG_BITS = 128
private const val TAG_BYTES = TAG_BITS / BITS_PER_BYTE
private const val HPKE_VERSION_LABEL = "HPKE-v1"
private const val KEM_SUITE_LABEL = "KEM"
private const val HPKE_SUITE_LABEL = "HPKE"
private const val EAE_PRK_LABEL = "eae_prk"
private const val SHARED_SECRET_LABEL = "shared_secret"
private const val PSK_ID_HASH_LABEL = "psk_id_hash"
private const val INFO_HASH_LABEL = "info_hash"
private const val SECRET_LABEL = "secret"
private const val KEY_LABEL = "key"
private const val BASE_NONCE_LABEL = "base_nonce"

private val EMPTY = ByteArray(0)
private val KEM_SUITE_ID =
    concat(
        KEM_SUITE_LABEL.toByteArray(Charsets.US_ASCII),
        twoByteBigEndian(KEM_ID),
    )
