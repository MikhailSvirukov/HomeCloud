package dev.homecloud.android.setup

import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/**
 * The setup values that survive a restart (`FR-AND-040 / 2`, `FR-AND-042 / 1`): the server host and
 * port, the account login and the chosen mode. It never carries the password.
 */
internal data class SetupConfiguration(
    val host: String,
    val port: Int,
    val login: String,
    val mode: SetupMode,
)

/** Persists the setup values that are safe to keep across restarts. */
internal interface SetupConfigStore {
    fun load(): SetupConfiguration?

    fun save(configuration: SetupConfiguration)
}

/**
 * [SetupConfigStore] backed by one UTF-8 text file.
 *
 * The file holds exactly four `key=value` lines in the order `host`, `port`, `login`, `mode`. The
 * mode is the [SetupMode] name. The login is restricted to letters, digits, dot, underscore and
 * hyphen, and the host is an IP literal, so no value needs escaping; the password has no line at
 * all. A write builds a temporary sibling and renames it over the target, using an atomic move
 * where the file system supports it, so a partial file is never read back; the caller owns the
 * directory and a missing directory is not created. Reading an absent file or content that does
 * not parse into a valid configuration yields no configuration instead of an exception. Not
 * thread-safe; access must stay on one thread or be externally synchronized.
 */
internal class FileSetupConfigStore(
    private val file: File,
) : SetupConfigStore {
    override fun load(): SetupConfiguration? {
        if (!file.exists()) return null
        val content =
            try {
                file.readText(Charsets.UTF_8)
            } catch (failure: IOException) {
                return null
            }
        return parse(content)
    }

    override fun save(configuration: SetupConfiguration) {
        val target = file.absoluteFile
        val directory =
            target.parentFile
                ?: throw IOException("Could not resolve the setup configuration directory")
        val temporary = File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
        try {
            temporary.writeText(format(configuration), Charsets.UTF_8)
            moveOver(temporary, target)
        } finally {
            if (temporary.exists()) temporary.delete()
        }
    }

    private fun format(configuration: SetupConfiguration): String =
        listOf(
            HOST_KEY to configuration.host,
            PORT_KEY to configuration.port.toString(),
            LOGIN_KEY to configuration.login,
            MODE_KEY to configuration.mode.name,
        ).joinToString(NEWLINE) { (key, value) -> "$key$KEY_SEPARATOR$value" }

    private fun parse(content: String): SetupConfiguration? {
        val values = parseLines(content) ?: return null
        val host = values[HOST_KEY] ?: return null
        val port = values[PORT_KEY]?.toIntOrNull() ?: return null
        val login = values[LOGIN_KEY] ?: return null
        val mode = values[MODE_KEY]?.let(::parseMode) ?: return null
        val draft =
            SetupDraft(
                host = host,
                port = port.toString(),
                login = login,
                password = UNUSED_PASSWORD_PROBE,
                mode = mode,
            )
        if (!SetupValidator.validate(draft).isValid) return null
        return SetupConfiguration(host = host, port = port, login = login, mode = mode)
    }

    private fun parseLines(content: String): Map<String, String>? {
        val lines =
            content.split(NEWLINE).let { split ->
                if (split.lastOrNull() == EMPTY_LINE) split.dropLast(1) else split
            }
        if (lines.size != COLUMN_COUNT) return null
        val values = LinkedHashMap<String, String>()
        for (line in lines) {
            val separator = line.indexOf(KEY_SEPARATOR)
            if (separator <= 0) return null
            val key = line.substring(0, separator)
            val value = line.substring(separator + 1)
            if (key !in COLUMNS || values.put(key, value) != null) return null
        }
        return values
    }

    private fun parseMode(value: String): SetupMode? = SetupMode.entries.firstOrNull { mode -> mode.name == value }

    private fun moveOver(
        source: File,
        target: File,
    ) {
        try {
            Files.move(
                source.toPath(),
                target.toPath(),
                StandardCopyOption.REPLACE_EXISTING,
                StandardCopyOption.ATOMIC_MOVE,
            )
        } catch (unsupported: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private companion object {
        const val KEY_SEPARATOR = '='
        const val NEWLINE = "\n"
        const val EMPTY_LINE = ""
        const val COLUMN_COUNT = 4

        const val HOST_KEY = "host"
        const val PORT_KEY = "port"
        const val LOGIN_KEY = "login"
        const val MODE_KEY = "mode"

        const val TEMPORARY_FILE_PREFIX = "setup-config-"
        const val TEMPORARY_FILE_SUFFIX = ".tmp"

        // The stored format has no password line; the shared field rules would reject an empty
        // probe for a configuration that is otherwise valid, so loading validates with this value.
        const val UNUSED_PASSWORD_PROBE = "unused"

        val COLUMNS = setOf(HOST_KEY, PORT_KEY, LOGIN_KEY, MODE_KEY)
    }
}
