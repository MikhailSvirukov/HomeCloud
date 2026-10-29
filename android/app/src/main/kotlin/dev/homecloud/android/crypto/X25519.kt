package dev.homecloud.android.crypto

import org.bouncycastle.crypto.CryptoServicesRegistrar
import org.bouncycastle.crypto.agreement.X25519Agreement
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters

/**
 * An X25519 key agreement key pair in its fixed 32-byte private and public key encoding.
 */
internal class X25519KeyPair(
    val privateKey: ByteArray,
    val publicKey: ByteArray,
)

/**
 * X25519 key agreement used by HPKE (`INT-NET-027`).
 *
 * Keys use the fixed 32-byte encoding. Agreement rejects malformed keys and the all-zero shared
 * secret produced by a low-order peer point.
 */
internal object X25519 {
    fun generate(): X25519KeyPair {
        val privateKey = X25519PrivateKeyParameters(CryptoServicesRegistrar.getSecureRandom())
        return X25519KeyPair(
            privateKey = privateKey.encoded,
            publicKey = privateKey.generatePublicKey().encoded,
        )
    }

    fun publicKeyOf(privateKey: ByteArray): ByteArray {
        requireKeyLength(privateKey, "X25519 private key")
        return X25519PrivateKeyParameters(privateKey, 0).generatePublicKey().encoded
    }

    fun sharedSecret(
        privateKey: ByteArray,
        peerPublicKey: ByteArray,
    ): ByteArray {
        requireKeyLength(privateKey, "X25519 private key")
        requireKeyLength(peerPublicKey, "X25519 peer public key")
        val agreement = X25519Agreement()
        agreement.init(X25519PrivateKeyParameters(privateKey, 0))
        val sharedSecret = ByteArray(agreement.agreementSize)
        try {
            agreement.calculateAgreement(X25519PublicKeyParameters(peerPublicKey, 0), sharedSecret, 0)
        } catch (_: IllegalArgumentException) {
            throw CryptoException("X25519 shared secret cannot be computed")
        } catch (_: IllegalStateException) {
            // BouncyCastle reports an all-zero agreement with this runtime exception.
            throw CryptoException("X25519 shared secret must not be all zero")
        }
        if (sharedSecret.all { byte -> byte == 0.toByte() }) {
            throw CryptoException("X25519 shared secret must not be all zero")
        }
        return sharedSecret
    }

    private fun requireKeyLength(
        key: ByteArray,
        name: String,
    ) {
        if (key.size != KEY_BYTES) {
            throw CryptoException("$name must be $KEY_BYTES bytes")
        }
    }
}

private const val KEY_BYTES = 32
