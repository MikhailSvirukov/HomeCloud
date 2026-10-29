package dev.homecloud.android.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

/**
 * Asserts the persistence rules of `FR-AND-040 / 2` and `FR-AND-042 / 1`: the host, port, login and
 * mode survive a restart, nothing is loaded from an absent or malformed file, the password has no
 * place in the file at all, and a later save replaces the previous content.
 */
class FileSetupConfigStoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun `saved configuration round trips every persisted field`() {
        val store = FileSetupConfigStore(configFile())
        val configuration = CONFIGURATION

        store.save(configuration)

        assertEquals(configuration, store.load())
    }

    @Test
    fun `load applies the supplied limits to the stored login`() {
        val store = storeWithContent(content(HOST_LINE, PORT_LINE, LONG_LOGIN_LINE, MODE_LINE))

        assertEquals(CONFIGURATION.copy(login = LONG_LOGIN), store.load(WIDE_LIMITS))
        assertNull(store.load())
    }

    @Test
    fun `missing file loads no configuration`() {
        val store = FileSetupConfigStore(configFile())

        assertNull(store.load())
    }

    @Test
    fun `file with a missing line loads no configuration`() {
        val store = storeWithContent(content(HOST_LINE, PORT_LINE, LOGIN_LINE))

        assertNull(store.load())
    }

    @Test
    fun `file with an unknown key loads no configuration`() {
        val store = storeWithContent(content(HOST_LINE, PORT_LINE, UNKNOWN_KEY_LINE, MODE_LINE))

        assertNull(store.load())
    }

    @Test
    fun `file with a port outside the valid range loads no configuration`() {
        val store = storeWithContent(content(HOST_LINE, OUT_OF_RANGE_PORT_LINE, LOGIN_LINE, MODE_LINE))

        assertNull(store.load())
    }

    @Test
    fun `file with an unknown mode loads no configuration`() {
        val store = storeWithContent(content(HOST_LINE, PORT_LINE, LOGIN_LINE, UNKNOWN_MODE_LINE))

        assertNull(store.load())
    }

    @Test
    fun `file with more lines than the persisted columns loads no configuration`() {
        val store = storeWithContent(content(HOST_LINE, PORT_LINE, LOGIN_LINE, MODE_LINE, EXTRA_LINE))

        assertNull(store.load())
    }

    @Test
    fun `saved file has no password entry`() {
        val file = configFile()
        val store = FileSetupConfigStore(file)

        store.save(CONFIGURATION)

        val text = file.readText(Charsets.UTF_8)
        assertFalse(text, text.contains(PASSWORD_KEY))
    }

    @Test
    fun `saving over an existing file replaces the previous configuration`() {
        val store = FileSetupConfigStore(configFile())
        val replacement = CONFIGURATION.copy(host = "2001:db8::1", login = "second-user", mode = SetupMode.CONNECT)

        store.save(CONFIGURATION)
        store.save(replacement)

        assertEquals(replacement, store.load())
    }

    private fun configFile(): File = File(temporaryFolder.newFolder(), FILE_NAME)

    private fun storeWithContent(raw: String): FileSetupConfigStore {
        val file = configFile()
        file.writeText(raw, Charsets.UTF_8)
        return FileSetupConfigStore(file)
    }

    private fun content(vararg lines: String): String = lines.joinToString(NEWLINE)

    private companion object {
        const val FILE_NAME = "setup-config.txt"
        const val NEWLINE = "\n"

        const val HOST_LINE = "host=192.0.2.10"
        const val PORT_LINE = "port=8443"
        const val LOGIN_LINE = "login=user"
        const val MODE_LINE = "mode=REGISTER"

        /** One byte above the default login maximum, so only a wider [SetupLimits] accepts it. */
        val LONG_LOGIN = "u".repeat(SetupValidator.defaultLimits.loginMaxBytes + 1)
        val LONG_LOGIN_LINE = "login=$LONG_LOGIN"
        val WIDE_LIMITS = SetupLimits(loginMaxBytes = LONG_LOGIN.length)
        const val UNKNOWN_KEY_LINE = "user=user"
        const val OUT_OF_RANGE_PORT_LINE = "port=70000"
        const val UNKNOWN_MODE_LINE = "mode=DELETE"
        const val EXTRA_LINE = "extra=value"
        const val PASSWORD_KEY = "password"

        val CONFIGURATION =
            SetupConfiguration(
                host = "192.0.2.10",
                port = 8443,
                login = "user",
                mode = SetupMode.REGISTER,
            )
    }
}
