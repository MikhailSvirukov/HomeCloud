package dev.homecloud.android.crypto

import org.bouncycastle.crypto.CryptoServicesRegistrar
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer

/**
 * An Ed25519 signing key pair in its fixed 32-byte seed and 32-byte public key encoding.
 *
 * Two key pairs are equal when both of their key byte arrays are equal.
 */
internal class Ed25519KeyPair(
    val privateKey: ByteArray,
    val publicKey: ByteArray,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is Ed25519KeyPair) {
            return false
        }
        return privateKey.contentEquals(other.privateKey) &&
            publicKey.contentEquals(other.publicKey)
    }

    override fun hashCode(): Int = 31 * privateKey.contentHashCode() + publicKey.contentHashCode()
}

/**
 * Ed25519 signing and verification (`INT-NET-018`, `INT-NET-019`, `INT-NET-028`).
 *
 * Keys use the fixed 32-byte seed and 32-byte public key encodings, and signatures are 64 bytes.
 * Verification treats a signature or public key of any other size as invalid and reports failure,
 * while signing and public key derivation reject a private key that is not 32 bytes.
 */
internal object Ed25519 {
    fun generate(): Ed25519KeyPair {
        val privateKey = Ed25519PrivateKeyParameters(CryptoServicesRegistrar.getSecureRandom())
        return Ed25519KeyPair(
            privateKey = privateKey.encoded,
            publicKey = privateKey.generatePublicKey().encoded,
        )
    }

    fun publicKeyOf(privateKey: ByteArray): ByteArray {
        requirePrivateKey(privateKey)
        return Ed25519PrivateKeyParameters(privateKey, 0).generatePublicKey().encoded
    }

    fun sign(
        privateKey: ByteArray,
        message: ByteArray,
    ): ByteArray {
        requirePrivateKey(privateKey)
        val signer = Ed25519Signer()
        signer.init(true, Ed25519PrivateKeyParameters(privateKey, 0))
        signer.update(message, 0, message.size)
        return signer.generateSignature()
    }

    fun verify(
        publicKey: ByteArray,
        message: ByteArray,
        signature: ByteArray,
    ): Boolean {
        if (publicKey.size != PUBLIC_KEY_BYTES || signature.size != SIGNATURE_BYTES) {
            return false
        }
        return try {
            val verifier = Ed25519Signer()
            verifier.init(false, Ed25519PublicKeyParameters(publicKey, 0))
            verifier.update(message, 0, message.size)
            verifier.verifySignature(signature)
        } catch (_: IllegalArgumentException) {
            false
        }
    }

    private fun requirePrivateKey(privateKey: ByteArray) {
        if (privateKey.size != PRIVATE_KEY_BYTES) {
            throw CryptoException("Ed25519 private key must be $PRIVATE_KEY_BYTES bytes")
        }
    }
}

private const val PRIVATE_KEY_BYTES = 32
private const val PUBLIC_KEY_BYTES = 32
private const val SIGNATURE_BYTES = 64
