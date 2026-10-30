package dev.homecloud.android.registration

import dev.homecloud.android.identity.IdentityCipher
import dev.homecloud.android.identity.IdentityStorageException
import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Base64

private val REGISTRATION_BASE64_ENCODER = Base64.getEncoder()
private val REGISTRATION_BASE64_DECODER = Base64.getDecoder()

private fun encodeBase64(value: ByteArray): String = REGISTRATION_BASE64_ENCODER.encodeToString(value)

private fun decodeBase64(encoded: String): ByteArray? =
    try {
        REGISTRATION_BASE64_DECODER
            .decode(encoded)
            .takeIf { decoded -> REGISTRATION_BASE64_ENCODER.encodeToString(decoded) == encoded }
    } catch (_: IllegalArgumentException) {
        null
    }

/**
 * Persists the pending registration so an interrupted attempt can be retried with the same logical
 * request after a restart (`FR-AND-053 / 1`, `FR-AND-054 / 1`, `FR-AND-055 / 1`).
 *
 * Reading and writing are blocking file I/O, so callers run them off the main thread.
 */
internal interface PendingRegistrationStore {
    /** Produces the stored pending registration, or no record when none can be recovered. */
    fun load(): PendingRegistration?

    /** Replaces the stored pending registration in full. */
    fun save(record: PendingRegistration)

    /** Removes the stored pending registration and succeeds when none is stored. */
    fun clear()
}

/**
 * [PendingRegistrationStore] backed by one app-private encrypted envelope (`FR-AND-022 / 1`,
 * `FR-AND-023 / 2`, `FR-AND-026 / 2`, `FR-AND-053 / 1`).
 *
 * The file holds one `payload=<base64>` line, optionally followed by a single newline. The payload
 * is the cipher output for this text format, one field per line:
 *
 * - `requestId=<base64>`: the 16-byte request id.
 * - `login=<text>`: the non-empty login.
 * - `password=<base64>`: the UTF-8 password bytes.
 * - `x25519PublicKey=<base64>` and `ed25519PublicKey=<base64>`: the 32-byte installation keys.
 * - `arguments=<base64>`: the encoded registration arguments (`INT-NET-058 / 1`).
 * - `startedAtMillis=<decimal>`: the attempt start time.
 *
 * The whole text is encrypted, so the password and the public keys never appear in plaintext on
 * disk. A non-exportable Android KeyStore key protects the envelope where [IdentityCipher] is the
 * key store cipher (`FR-AND-026 / 2`).
 *
 * Loading reports a record only when the envelope, the cipher output and the text format all
 * validate. Unreadable or inconsistent content is reported as no record so a new attempt can start.
 * Writing validates the record before encrypting it, then builds a temporary sibling before
 * renaming it over the target, using an atomic move where the file system supports it, so a partial
 * file is never read back. The caller owns the directory and a missing directory is not created.
 * Not thread-safe; access must stay on one thread or be externally synchronized.
 */
internal class FilePendingRegistrationStore(
    private val file: File,
    private val cipher: IdentityCipher,
) : PendingRegistrationStore {
    private val textFormat = PendingRegistrationTextFormat()

    override fun load(): PendingRegistration? =
        try {
            if (file.exists()) readRecord() else null
        } catch (_: Exception) {
            null
        }

    override fun save(record: PendingRegistration) {
        val plaintext = textFormat.format(record).toByteArray(Charsets.UTF_8)
        val payload = cipher.encrypt(plaintext)
        val envelope = PAYLOAD_PREFIX + encodeBase64(payload) + NEWLINE
        val target = file.absoluteFile
        val directory =
            target.parentFile
                ?: throw IOException("Could not resolve the pending registration directory")
        val temporary = File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
        try {
            temporary.writeText(envelope, Charsets.UTF_8)
            moveOver(temporary, target)
        } finally {
            if (temporary.exists()) {
                temporary.delete()
            }
        }
    }

    override fun clear() {
        if (file.exists()) {
            file.delete()
        }
    }

    private fun readRecord(): PendingRegistration? {
        val content = file.readText(Charsets.UTF_8)
        val payload = parseEnvelope(content) ?: return null
        val plaintext =
            try {
                cipher.decrypt(payload)
            } catch (_: IdentityStorageException) {
                return null
            }
        return textFormat.parse(String(plaintext, Charsets.UTF_8))
    }

    private fun parseEnvelope(content: String): ByteArray? {
        val lines = content.split(NEWLINE)
        val payloadLine =
            when {
                lines.size == 1 -> lines.first()
                lines.size == 2 && lines.last() == EMPTY_LINE -> lines.first()
                else -> return null
            }
        if (!payloadLine.startsWith(PAYLOAD_PREFIX)) {
            return null
        }
        val encoded = payloadLine.removePrefix(PAYLOAD_PREFIX)
        if (encoded.isEmpty()) {
            return null
        }
        return decodeBase64(encoded)
    }

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
        } catch (_: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private companion object {
        const val TEMPORARY_FILE_PREFIX = "pending-registration-"
        const val TEMPORARY_FILE_SUFFIX = ".tmp"
        const val PAYLOAD_PREFIX = "payload="
        const val NEWLINE = "\n"
        const val EMPTY_LINE = ""
    }
}

/**
 * The text representation that [FilePendingRegistrationStore] encrypts before writing
 * (`FR-AND-053 / 1`).
 *
 * Every field is one `key=value` line in UTF-8. Byte values use canonical padded Base64, the login
 * uses its text form and the start time uses its decimal form. Writing refuses a record whose
 * request id, keys, login or password cannot form a valid representation, and reading refuses
 * content that does not form one.
 */
internal class PendingRegistrationTextFormat {
    fun format(record: PendingRegistration): String {
        validate(record)
        return listOf(
            line(KEY_REQUEST_ID, record.requestId),
            textLine(KEY_LOGIN, record.login),
            line(KEY_PASSWORD, record.password.toByteArray(Charsets.UTF_8)),
            line(KEY_X25519_PUBLIC_KEY, record.x25519PublicKey),
            line(KEY_ED25519_PUBLIC_KEY, record.ed25519PublicKey),
            line(KEY_ARGUMENTS, record.arguments),
            textLine(KEY_STARTED_AT_MILLIS, record.startedAtMillis.toString()),
        ).joinToString(NEWLINE)
    }

    fun parse(content: String): PendingRegistration? {
        val values = parseLines(content) ?: return null

        val requestId =
            values[KEY_REQUEST_ID]
                ?.let(::decodeBase64)
                ?.takeIf { value -> value.size == REQUEST_ID_BYTES }
                ?: return null
        val login = values[KEY_LOGIN]?.takeIf { value -> value.isNotEmpty() } ?: return null
        val passwordBytes = values[KEY_PASSWORD]?.let(::decodeBase64) ?: return null
        val password = String(passwordBytes, Charsets.UTF_8)
        if (password.isEmpty() || !password.toByteArray(Charsets.UTF_8).contentEquals(passwordBytes)) {
            return null
        }
        val x25519PublicKey =
            values[KEY_X25519_PUBLIC_KEY]
                ?.let(::decodeBase64)
                ?.takeIf { value -> value.size == PUBLIC_KEY_BYTES }
                ?: return null
        val ed25519PublicKey =
            values[KEY_ED25519_PUBLIC_KEY]
                ?.let(::decodeBase64)
                ?.takeIf { value -> value.size == PUBLIC_KEY_BYTES }
                ?: return null
        val arguments = values[KEY_ARGUMENTS]?.let(::decodeBase64) ?: return null
        val startedAtMillisText = values[KEY_STARTED_AT_MILLIS] ?: return null
        val startedAtMillis = startedAtMillisText.toLongOrNull() ?: return null
        if (startedAtMillis.toString() != startedAtMillisText) {
            return null
        }

        return PendingRegistration(
            requestId = requestId,
            login = login,
            password = password,
            x25519PublicKey = x25519PublicKey,
            ed25519PublicKey = ed25519PublicKey,
            arguments = arguments,
            startedAtMillis = startedAtMillis,
        )
    }

    private fun validate(record: PendingRegistration) {
        if (record.requestId.size != REQUEST_ID_BYTES) throw IllegalArgumentException(KEY_REQUEST_ID)
        if (record.login.isEmpty() || record.login.contains(NEWLINE)) throw IllegalArgumentException(KEY_LOGIN)
        if (record.password.isEmpty()) throw IllegalArgumentException(KEY_PASSWORD)
        requireKeyLength(KEY_X25519_PUBLIC_KEY, record.x25519PublicKey)
        requireKeyLength(KEY_ED25519_PUBLIC_KEY, record.ed25519PublicKey)
    }

    private fun requireKeyLength(
        field: String,
        value: ByteArray,
    ) {
        if (value.size != PUBLIC_KEY_BYTES) throw IllegalArgumentException(field)
    }

    private fun line(
        key: String,
        value: ByteArray,
    ): String = "$key$KEY_SEPARATOR${encodeBase64(value)}"

    private fun textLine(
        key: String,
        value: String,
    ): String = "$key$KEY_SEPARATOR$value"

    private fun parseLines(content: String): Map<String, String>? {
        val lines =
            content.split(NEWLINE).let { split ->
                if (split.lastOrNull() == EMPTY_LINE) split.dropLast(1) else split
            }
        val values = LinkedHashMap<String, String>()
        for (line in lines) {
            val separator = line.indexOf(KEY_SEPARATOR)
            if (separator <= 0) return null
            val key = line.substring(0, separator)
            if (key !in KEYS) return null
            if (values.put(key, line.substring(separator + 1)) != null) return null
        }
        if (values.keys != KEYS) return null
        return values
    }

    private companion object {
        const val KEY_SEPARATOR = '='
        const val NEWLINE = "\n"
        const val EMPTY_LINE = ""

        const val REQUEST_ID_BYTES = 16
        const val PUBLIC_KEY_BYTES = 32

        const val KEY_REQUEST_ID = "requestId"
        const val KEY_LOGIN = "login"
        const val KEY_PASSWORD = "password"
        const val KEY_X25519_PUBLIC_KEY = "x25519PublicKey"
        const val KEY_ED25519_PUBLIC_KEY = "ed25519PublicKey"
        const val KEY_ARGUMENTS = "arguments"
        const val KEY_STARTED_AT_MILLIS = "startedAtMillis"

        val KEYS =
            setOf(
                KEY_REQUEST_ID,
                KEY_LOGIN,
                KEY_PASSWORD,
                KEY_X25519_PUBLIC_KEY,
                KEY_ED25519_PUBLIC_KEY,
                KEY_ARGUMENTS,
                KEY_STARTED_AT_MILLIS,
            )
    }
}
