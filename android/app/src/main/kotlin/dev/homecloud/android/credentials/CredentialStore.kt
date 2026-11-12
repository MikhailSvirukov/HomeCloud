package dev.homecloud.android.credentials

import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Base64

/**
 * Signals that the stored account credential could not be read, written or removed
 * (`FR-AND-022 / 1`, `FR-AND-024 / 1`).
 *
 * The message never contains the login, password or path, so a caller can report the failure
 * without exposing the credential.
 */
internal class CredentialStorageException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/** The account credential kept for background requests (`FR-AND-022 / 1`). */
internal data class StoredCredential(
    val login: String,
    val password: String,
) {
    /** Keeps the password out of any message that prints this value (`FR-AND-023 / 2`). */
    override fun toString(): String = "StoredCredential(login=redacted, password=redacted)"
}

/**
 * Reads and writes the single account credential used by background requests (`FR-AND-022 / 1`).
 *
 * A credential that cannot be opened because its key is unavailable is reported as
 * [CredentialStorageException] so the caller can request the password again (`FR-AND-024 / 1`).
 */
internal interface CredentialStore {
    /** Reads the stored credential, reporting absence only when none is stored (`FR-AND-024 / 1`). */
    fun load(): StoredCredential?

    /** Encrypts and stores the credential, replacing any earlier one (`FR-AND-022 / 1`). */
    fun save(credential: StoredCredential)

    /** Removes the stored credential when present. */
    fun clear()
}

/**
 * [CredentialStore] backed by one app-private encrypted envelope (`FR-AND-022 / 1`,
 * `FR-AND-024 / 1`, `FR-AND-025 / 1`).
 *
 * The file holds one `payload=<base64>` line followed by a single newline. The payload is the
 * cipher output for the binary credential: a four-byte big-endian login length, the login bytes and
 * the password bytes. A non-exportable Android KeyStore AES-256-GCM key protects the envelope, so
 * neither the login nor the password appears in plaintext on disk (`FR-AND-022 / 1`).
 *
 * A missing file means no credential is stored. Unreadable, malformed or undecryptable content is
 * reported as [CredentialStorageException] so the caller can request the password again
 * (`FR-AND-024 / 1`); saving the re-entered credential succeeds without touching the installation
 * identity (`FR-AND-025 / 1`). Writing validates the credential, encrypts it and replaces the target
 * atomically from a temporary sibling, creating the directory when needed. Reading and writing are
 * blocking file I/O, so callers run them off the main thread. Not thread-safe; access must stay on
 * one thread or be externally synchronized.
 */
internal class FileCredentialStore(
    private val file: File,
    private val cipher: CredentialCipher,
) : CredentialStore {
    override fun load(): StoredCredential? =
        try {
            if (file.exists()) readCredential() else null
        } catch (exception: IOException) {
            throw CredentialStorageException(READ_FAILED_MESSAGE, exception)
        } catch (exception: SecurityException) {
            throw CredentialStorageException(READ_FAILED_MESSAGE, exception)
        }

    override fun save(credential: StoredCredential) {
        requireComplete(credential)
        val payload = cipher.encrypt(encodePayload(credential))
        val envelope = PAYLOAD_PREFIX + BASE64_ENCODER.encodeToString(payload) + NEWLINE
        val target = file.absoluteFile
        val directory = target.parentFile ?: throw CredentialStorageException(WRITE_FAILED_MESSAGE)
        try {
            if (!directory.isDirectory) {
                directory.mkdirs()
                if (!directory.isDirectory) throw CredentialStorageException(WRITE_FAILED_MESSAGE)
            }
            val temporary = File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
            try {
                temporary.writeText(envelope, Charsets.UTF_8)
                moveOver(temporary, target)
            } finally {
                if (temporary.exists()) temporary.delete()
            }
        } catch (exception: IOException) {
            throw CredentialStorageException(WRITE_FAILED_MESSAGE, exception)
        } catch (exception: SecurityException) {
            throw CredentialStorageException(WRITE_FAILED_MESSAGE, exception)
        }
    }

    override fun clear() {
        try {
            if (file.exists() && !file.delete()) {
                throw CredentialStorageException(CLEAR_FAILED_MESSAGE)
            }
        } catch (exception: SecurityException) {
            throw CredentialStorageException(CLEAR_FAILED_MESSAGE, exception)
        }
    }

    private fun readCredential(): StoredCredential {
        val content = file.readText(Charsets.UTF_8)
        val payload = parseEnvelope(content) ?: throw CredentialStorageException(READ_FAILED_MESSAGE)
        return decodePayload(cipher.decrypt(payload))
    }

    private fun requireComplete(credential: StoredCredential) {
        if (credential.login.isEmpty() || credential.password.isEmpty()) {
            throw IllegalArgumentException(INVALID_CREDENTIAL_MESSAGE)
        }
    }

    private fun encodePayload(credential: StoredCredential): ByteArray {
        val login = credential.login.toByteArray(Charsets.UTF_8)
        val password = credential.password.toByteArray(Charsets.UTF_8)
        return ByteBuffer
            .allocate(LOGIN_LENGTH_BYTES + login.size + password.size)
            .putInt(login.size)
            .put(login)
            .put(password)
            .array()
    }

    private fun decodePayload(payload: ByteArray): StoredCredential {
        if (payload.size <= LOGIN_LENGTH_BYTES) throw CredentialStorageException(READ_FAILED_MESSAGE)
        val buffer = ByteBuffer.wrap(payload)
        val loginLength = buffer.int
        if (loginLength <= 0 || loginLength >= buffer.remaining()) {
            throw CredentialStorageException(READ_FAILED_MESSAGE)
        }
        val loginBytes = ByteArray(loginLength).also(buffer::get)
        val passwordBytes = ByteArray(buffer.remaining()).also(buffer::get)
        val login = decodeUtf8(loginBytes) ?: throw CredentialStorageException(READ_FAILED_MESSAGE)
        val password = decodeUtf8(passwordBytes) ?: throw CredentialStorageException(READ_FAILED_MESSAGE)
        if (login.isEmpty() || password.isEmpty()) throw CredentialStorageException(READ_FAILED_MESSAGE)
        return StoredCredential(login = login, password = password)
    }

    private fun decodeUtf8(bytes: ByteArray): String? {
        val text = String(bytes, Charsets.UTF_8)
        return text.takeIf { decoded -> decoded.toByteArray(Charsets.UTF_8).contentEquals(bytes) }
    }

    private fun parseEnvelope(content: String): ByteArray? {
        val lines = content.split(NEWLINE)
        val payloadLine =
            when {
                lines.size == 1 -> lines.first()
                lines.size == 2 && lines.last() == EMPTY_LINE -> lines.first()
                else -> return null
            }
        if (!payloadLine.startsWith(PAYLOAD_PREFIX)) return null
        val encoded = payloadLine.removePrefix(PAYLOAD_PREFIX)
        if (encoded.isEmpty()) return null
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
}

private fun decodeBase64(encoded: String): ByteArray? =
    try {
        BASE64_DECODER.decode(encoded).takeIf { decoded -> BASE64_ENCODER.encodeToString(decoded) == encoded }
    } catch (_: IllegalArgumentException) {
        null
    }

private val BASE64_ENCODER = Base64.getEncoder()
private val BASE64_DECODER = Base64.getDecoder()

private const val PAYLOAD_PREFIX = "payload="
private const val NEWLINE = "\n"
private const val EMPTY_LINE = ""
private const val LOGIN_LENGTH_BYTES = 4

private const val TEMPORARY_FILE_PREFIX = "account-credential-"
private const val TEMPORARY_FILE_SUFFIX = ".tmp"

private const val INVALID_CREDENTIAL_MESSAGE = "A stored credential needs a non-empty login and password"
private const val READ_FAILED_MESSAGE = "Could not read the stored account credential"
private const val WRITE_FAILED_MESSAGE = "Could not store the account credential"
private const val CLEAR_FAILED_MESSAGE = "Could not remove the stored account credential"
