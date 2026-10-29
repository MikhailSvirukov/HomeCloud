package dev.homecloud.android.setup

/**
 * Which setup operation the user chose (`FR-AND-042 / 1`): register a new account or connect this
 * installation to an existing one.
 */
internal enum class SetupMode {
    REGISTER,
    CONNECT,
}

/**
 * The setup values as they are currently typed. The draft lives in memory only; the password is
 * never written to disk or to a log (`FR-AND-041 / 2`).
 */
internal data class SetupDraft(
    val host: String = "",
    val port: String = "",
    val login: String = "",
    val password: String = "",
    val mode: SetupMode = SetupMode.REGISTER,
)

/** The setup fields that can carry an error. */
internal enum class SetupField {
    HOST,
    PORT,
    LOGIN,
    PASSWORD,
}

/** The first error a setup field can show. */
internal enum class SetupError {
    REQUIRED,
    NOT_AN_IP_LITERAL,
    PORT_OUT_OF_RANGE,
    LOGIN_CHARACTERS,
    LOGIN_TOO_SHORT,
    LOGIN_TOO_LONG,
    PASSWORD_TOO_SHORT,
    PASSWORD_TOO_LONG,
}

/**
 * The configured credential length bounds (`CFG-CL-029 / 1`). This object is the configuration
 * input of the setup validator; every bound is a UTF-8 byte count. The default values are the
 * chosen `OPEN-017` client defaults (1/64 for the login and 1/128 for the password) until the
 * configuration supplies other values.
 */
internal data class SetupLimits(
    val loginMinBytes: Int = 1,
    val loginMaxBytes: Int = 64,
    val passwordMinBytes: Int = 1,
    val passwordMaxBytes: Int = 128,
) {
    init {
        require(loginMinBytes > 0) { "loginMinBytes must be positive" }
        require(loginMaxBytes > 0) { "loginMaxBytes must be positive" }
        require(passwordMinBytes > 0) { "passwordMinBytes must be positive" }
        require(passwordMaxBytes > 0) { "passwordMaxBytes must be positive" }
        require(loginMinBytes <= loginMaxBytes) { "loginMinBytes must not exceed loginMaxBytes" }
        require(passwordMinBytes <= passwordMaxBytes) { "passwordMinBytes must not exceed passwordMaxBytes" }
    }
}

/** The errors of one draft, at most one per field. */
internal data class SetupValidation(
    val errors: Map<SetupField, SetupError>,
) {
    /** Whether no field carries an error. */
    val isValid: Boolean
        get() = errors.isEmpty()
}

/**
 * Validates the setup draft against the v1 field rules (`FR-AND-040 / 2`, `FR-AND-041 / 2`,
 * `CFG-CL-029 / 1`). The host must be one canonical IP literal, the port must be in the TCP range,
 * an empty login is required, a login with any character outside the permitted ASCII set is
 * rejected, and a login is bounded by its configured UTF-8 byte length. An empty password is
 * required, a password is only bounded by its configured UTF-8 byte length, and the entered bytes
 * are never trimmed or normalized (`wire-v1.md` section 5). Each field reports only its first
 * applicable error.
 *
 * The credential bounds come from [limits], the configuration input required by `CFG-CL-029 / 1`.
 * [defaultLimits] holds the chosen `OPEN-017` client defaults (1/64 for the login and 1/128 for the
 * password) and is used when no configuration has supplied other values.
 */
internal object SetupValidator {
    /** The `OPEN-017` client defaults, used until the configuration supplies [SetupLimits]. */
    val defaultLimits: SetupLimits = SetupLimits()

    fun validate(
        draft: SetupDraft,
        limits: SetupLimits = defaultLimits,
    ): SetupValidation {
        val errors = LinkedHashMap<SetupField, SetupError>()
        validateHost(draft.host)?.let { errors[SetupField.HOST] = it }
        validatePort(draft.port)?.let { errors[SetupField.PORT] = it }
        validateLogin(draft.login, limits)?.let { errors[SetupField.LOGIN] = it }
        validatePassword(draft.password, limits)?.let { errors[SetupField.PASSWORD] = it }
        return SetupValidation(errors)
    }

    /**
     * Validates the values that a persisted configuration holds against [limits]
     * (`FR-AND-040 / 2`, `CFG-CL-029 / 1`): the host, port and login only, because a stored
     * configuration never carries a password.
     */
    fun validateStored(
        host: String,
        port: Int,
        login: String,
        limits: SetupLimits = defaultLimits,
    ): SetupValidation {
        val errors = LinkedHashMap<SetupField, SetupError>()
        validateHost(host)?.let { errors[SetupField.HOST] = it }
        validatePort(port.toString())?.let { errors[SetupField.PORT] = it }
        validateLogin(login, limits)?.let { errors[SetupField.LOGIN] = it }
        return SetupValidation(errors)
    }

    private fun validateHost(host: String): SetupError? =
        when {
            host.isBlank() -> SetupError.REQUIRED
            !HostLiteral.isCanonical(host) -> SetupError.NOT_AN_IP_LITERAL
            else -> null
        }

    private fun validatePort(port: String): SetupError? {
        if (port.isBlank()) return SetupError.REQUIRED
        val value = port.toIntOrNull() ?: return SetupError.PORT_OUT_OF_RANGE
        return if (value in MIN_PORT..MAX_PORT) null else SetupError.PORT_OUT_OF_RANGE
    }

    private fun validateLogin(
        login: String,
        limits: SetupLimits,
    ): SetupError? {
        val byteLength = login.encodeToByteArray().size
        return when {
            login.isEmpty() -> SetupError.REQUIRED
            !LOGIN_PATTERN.matches(login) -> SetupError.LOGIN_CHARACTERS
            byteLength < limits.loginMinBytes -> SetupError.LOGIN_TOO_SHORT
            byteLength > limits.loginMaxBytes -> SetupError.LOGIN_TOO_LONG
            else -> null
        }
    }

    private fun validatePassword(
        password: String,
        limits: SetupLimits,
    ): SetupError? {
        val byteLength = password.encodeToByteArray().size
        return when {
            password.isEmpty() -> SetupError.REQUIRED
            byteLength < limits.passwordMinBytes -> SetupError.PASSWORD_TOO_SHORT
            byteLength > limits.passwordMaxBytes -> SetupError.PASSWORD_TOO_LONG
            else -> null
        }
    }

    private const val MIN_PORT = 1
    private const val MAX_PORT = 65535

    private val LOGIN_PATTERN = Regex("[A-Za-z0-9._-]+")
}
