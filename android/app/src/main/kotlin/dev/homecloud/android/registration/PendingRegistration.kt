package dev.homecloud.android.registration

import dev.homecloud.android.protocol.CborEncoder
import dev.homecloud.android.protocol.CborValue

/**
 * One durable primary-registration attempt that can be repeated with the same logical request
 * (`FR-AND-053 / 1`, `FR-AND-054 / 1`, `FR-AND-055 / 1`).
 *
 * The record carries the request id, login, installation public keys, encoded operation arguments
 * and start time that identify the attempt. The password stays in memory and is written only inside
 * the encrypted envelope of [FilePendingRegistrationStore] (`FR-AND-022 / 1`, `FR-AND-023 / 2`).
 *
 * The textual representation never contains the password or any key bytes.
 */
internal data class PendingRegistration(
    val requestId: ByteArray,
    val login: String,
    val password: String,
    val x25519PublicKey: ByteArray,
    val ed25519PublicKey: ByteArray,
    val arguments: ByteArray,
    val startedAtMillis: Long,
) {
    override fun toString(): String = "PendingRegistration(login=$login, startedAtMillis=$startedAtMillis)"

    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is PendingRegistration) {
            return false
        }
        return requestId.contentEquals(other.requestId) &&
            login == other.login &&
            password == other.password &&
            x25519PublicKey.contentEquals(other.x25519PublicKey) &&
            ed25519PublicKey.contentEquals(other.ed25519PublicKey) &&
            arguments.contentEquals(other.arguments) &&
            startedAtMillis == other.startedAtMillis
    }

    override fun hashCode(): Int {
        var result = requestId.contentHashCode()
        result = 31 * result + login.hashCode()
        result = 31 * result + password.hashCode()
        result = 31 * result + x25519PublicKey.contentHashCode()
        result = 31 * result + ed25519PublicKey.contentHashCode()
        result = 31 * result + arguments.contentHashCode()
        result = 31 * result + startedAtMillis.hashCode()
        return result
    }
}

/**
 * Builds the operation-arguments array of a primary-registration request (`INT-NET-058 / 1`).
 *
 * The array holds the installation X25519 public key first and the installation Ed25519 public key
 * second, each as a 32-byte CBOR byte string in the deterministic encoding.
 */
internal object RegistrationArguments {
    fun encode(
        x25519PublicKey: ByteArray,
        ed25519PublicKey: ByteArray,
    ): ByteArray {
        requireKeyLength(X25519_FIELD, x25519PublicKey)
        requireKeyLength(ED25519_FIELD, ed25519PublicKey)
        return CborEncoder.encode(
            CborValue.Array(
                listOf(
                    CborValue.Bytes(x25519PublicKey),
                    CborValue.Bytes(ed25519PublicKey),
                ),
            ),
        )
    }

    private fun requireKeyLength(
        field: String,
        key: ByteArray,
    ) {
        if (key.size != PUBLIC_KEY_BYTES) throw IllegalArgumentException(field)
    }

    private const val PUBLIC_KEY_BYTES = 32
    private const val X25519_FIELD = "x25519PublicKey"
    private const val ED25519_FIELD = "ed25519PublicKey"
}
