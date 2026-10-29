package dev.homecloud.android.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the field rules of `FR-AND-040 / 2`, `FR-AND-041 / 2` and `CFG-CL-029 / 1`: an empty
 * draft is required everywhere, the host must be canonical, the port must be in the TCP range, the
 * login uses the permitted ASCII set and both credentials are bounded by configured UTF-8 byte
 * lengths rather than by constants.
 */
class SetupValidatorTest {
    @Test
    fun `empty draft reports a required error on every field`() {
        val validation = SetupValidator.validate(SetupDraft())

        val expected =
            mapOf(
                SetupField.HOST to SetupError.REQUIRED,
                SetupField.PORT to SetupError.REQUIRED,
                SetupField.LOGIN to SetupError.REQUIRED,
                SetupField.PASSWORD to SetupError.REQUIRED,
            )
        assertEquals(expected, validation.errors)
        assertFalse(validation.isValid)
    }

    @Test
    fun `valid draft reports no errors`() {
        val validation = SetupValidator.validate(validDraft())

        assertTrue(validation.errors.toString(), validation.isValid)
    }

    @Test
    fun `host that is not an ip literal is rejected`() {
        val validation = SetupValidator.validate(validDraft(host = "example.com"))

        assertEquals(SetupError.NOT_AN_IP_LITERAL, validation.errors[SetupField.HOST])
    }

    @Test
    fun `port zero is rejected`() {
        assertEquals(SetupError.PORT_OUT_OF_RANGE, portError("0"))
    }

    @Test
    fun `port above the maximum is rejected`() {
        assertEquals(SetupError.PORT_OUT_OF_RANGE, portError("65536"))
    }

    @Test
    fun `non numeric port is rejected`() {
        assertEquals(SetupError.PORT_OUT_OF_RANGE, portError("abc"))
    }

    @Test
    fun `login containing a space is rejected`() {
        val validation = SetupValidator.validate(validDraft(login = "user name"))

        assertEquals(SetupError.LOGIN_CHARACTERS, validation.errors[SetupField.LOGIN])
    }

    @Test
    fun `login containing a symbol is rejected`() {
        val validation = SetupValidator.validate(validDraft(login = "user@name"))

        assertEquals(SetupError.LOGIN_CHARACTERS, validation.errors[SetupField.LOGIN])
    }

    @Test
    fun `login of the default maximum ascii bytes is accepted`() {
        val login = "a".repeat(SetupValidator.defaultLimits.loginMaxBytes)
        val validation = SetupValidator.validate(validDraft(login = login))

        assertTrue(validation.errors.toString(), validation.isValid)
    }

    @Test
    fun `login of one byte above the default maximum is rejected`() {
        val login = "a".repeat(SetupValidator.defaultLimits.loginMaxBytes + 1)
        val validation = SetupValidator.validate(validDraft(login = login))

        assertEquals(SetupError.LOGIN_TOO_LONG, validation.errors[SetupField.LOGIN])
    }

    @Test
    fun `login of two utf8 bytes above the default maximum is rejected by the character set`() {
        val login = TWO_BYTE_CHARACTER.repeat(SetupValidator.defaultLimits.loginMaxBytes / 2 + 1)
        val validation = SetupValidator.validate(validDraft(login = login))

        assertEquals(SetupError.LOGIN_CHARACTERS, validation.errors[SetupField.LOGIN])
    }

    @Test
    fun `login below the configured minimum is rejected`() {
        val validation =
            SetupValidator.validate(
                validDraft(login = "u", password = CONFIGURED_PASSWORD),
                CONFIGURED_LIMITS,
            )

        assertEquals(mapOf(SetupField.LOGIN to SetupError.LOGIN_TOO_SHORT), validation.errors)
    }

    @Test
    fun `login above the configured maximum is rejected`() {
        val login = "u".repeat(CONFIGURED_LIMITS.loginMaxBytes + 1)
        val validation =
            SetupValidator.validate(
                validDraft(login = login, password = CONFIGURED_PASSWORD),
                CONFIGURED_LIMITS,
            )

        assertEquals(mapOf(SetupField.LOGIN to SetupError.LOGIN_TOO_LONG), validation.errors)
    }

    @Test
    fun `password of the default maximum utf8 bytes is accepted`() {
        val password = TWO_BYTE_CHARACTER.repeat(SetupValidator.defaultLimits.passwordMaxBytes / 2)
        val validation = SetupValidator.validate(validDraft(password = password))

        assertTrue(validation.errors.toString(), validation.isValid)
    }

    @Test
    fun `password of one utf8 byte above the default maximum is rejected`() {
        val password =
            TWO_BYTE_CHARACTER.repeat(SetupValidator.defaultLimits.passwordMaxBytes / 2) + "a"
        val validation = SetupValidator.validate(validDraft(password = password))

        assertEquals(SetupError.PASSWORD_TOO_LONG, validation.errors[SetupField.PASSWORD])
    }

    @Test
    fun `password below the configured minimum is rejected`() {
        val validation =
            SetupValidator.validate(
                validDraft(login = CONFIGURED_LOGIN, password = "p"),
                CONFIGURED_LIMITS,
            )

        assertEquals(mapOf(SetupField.PASSWORD to SetupError.PASSWORD_TOO_SHORT), validation.errors)
    }

    @Test
    fun `password above the configured maximum is rejected`() {
        val password = "p".repeat(CONFIGURED_LIMITS.passwordMaxBytes + 1)
        val validation =
            SetupValidator.validate(
                validDraft(login = CONFIGURED_LOGIN, password = password),
                CONFIGURED_LIMITS,
            )

        assertEquals(mapOf(SetupField.PASSWORD to SetupError.PASSWORD_TOO_LONG), validation.errors)
    }

    @Test
    fun `a non positive credential bound is rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            SetupLimits(loginMinBytes = 0)
        }
        assertThrows(IllegalArgumentException::class.java) {
            SetupLimits(passwordMaxBytes = 0)
        }
    }

    @Test
    fun `a credential minimum above its maximum is rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            SetupLimits(loginMinBytes = 5, loginMaxBytes = 4)
        }
        assertThrows(IllegalArgumentException::class.java) {
            SetupLimits(passwordMinBytes = 5, passwordMaxBytes = 4)
        }
    }

    private fun portError(port: String): SetupError? = SetupValidator.validate(validDraft(port = port)).errors[SetupField.PORT]

    private fun validDraft(
        host: String = CANONICAL_HOST,
        port: String = CANONICAL_PORT,
        login: String = CANONICAL_LOGIN,
        password: String = CANONICAL_PASSWORD,
    ): SetupDraft =
        SetupDraft(
            host = host,
            port = port,
            login = login,
            password = password,
        )

    private companion object {
        const val CANONICAL_HOST = "192.0.2.10"
        const val CANONICAL_PORT = "8443"
        const val CANONICAL_LOGIN = "user.name_1-x"
        const val CANONICAL_PASSWORD = "correct horse battery staple"

        /** A character whose UTF-8 encoding takes two bytes. */
        const val TWO_BYTE_CHARACTER = "é"

        /** Credentials inside [CONFIGURED_LIMITS], so only the tested field can fail. */
        const val CONFIGURED_LOGIN = "usr"
        const val CONFIGURED_PASSWORD = "pass"

        val CONFIGURED_LIMITS =
            SetupLimits(
                loginMinBytes = 2,
                loginMaxBytes = 4,
                passwordMinBytes = 2,
                passwordMaxBytes = 4,
            )
    }
}
