package dev.homecloud.android.registration

/**
 * Reports that a primary-registration request failed temporarily and may be retried with the same
 * request id (`FR-AND-054 / 1`).
 *
 * Messages identify the failing operation and never carry the password, key material or other
 * request contents (`FR-AND-023 / 2`).
 */
internal class RegistrationTransportException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/**
 * The logical fields of one primary-registration request (`INT-NET-058 / 1`).
 *
 * The password and key bytes never appear in the textual representation (`FR-AND-023 / 2`).
 */
internal data class RegistrationRequest(
    val requestId: ByteArray,
    val login: String,
    val password: String,
    val x25519PublicKey: ByteArray,
    val ed25519PublicKey: ByteArray,
    val arguments: ByteArray,
) {
    override fun toString(): String = "RegistrationRequest(login=$login)"
}

/** The successful installation id assigned by primary registration, if the server returned one (`INT-NET-059 / 1`). */
internal data class RegistrationResponse(
    val installationId: ByteArray?,
)

/**
 * Sends one primary-registration request to the dedicated server (`INT-NET-058 / 1`,
 * `INT-NET-059 / 1`).
 *
 * A temporary failure is reported as [RegistrationTransportException] so the caller can keep the
 * pending registration and retry it later (`FR-AND-054 / 1`).
 */
internal fun interface RegistrationTransport {
    fun register(request: RegistrationRequest): RegistrationResponse
}
