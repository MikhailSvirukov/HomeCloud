package dev.homecloud.android.identity

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import dev.homecloud.android.crypto.X25519KeyPair
import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Base64

private val BASE64_ENCODER = Base64.getEncoder()
private val BASE64_DECODER = Base64.getDecoder()

private fun decodeBase64(encoded: String): ByteArray? =
    try {
        BASE64_DECODER.decode(encoded).takeIf { decoded -> BASE64_ENCODER.encodeToString(decoded) == encoded }
    } catch (_: IllegalArgumentException) {
        null
    }

/**
 * [InstallationIdentityStore] backed by one app-private encrypted envelope (`FR-DED-022 / 1`,
 * `NFR-SEC-003 / 1`, `INT-NET-008 / 2`, `INT-NET-025 / 1`, `INT-NET-034 / 1`,
 * `FR-AND-026 / 2`).
 *
 * The file holds one `payload=<base64>` line, optionally followed by a single newline. The payload
 * is the cipher output for the existing `key=base64` text format: `signingPrivate`, `signingPublic`,
 * `agreementPrivate` and `agreementPublic` always, then `senderId` once registration has assigned
 * it (`INT-NET-008 / 2`), and `serverX25519` with `serverEd25519` once the first contact has pinned
 * the dedicated server keys (`INT-NET-025 / 1`, `INT-NET-034 / 1`). Private keys are never written
 * in plaintext: [KeystoreIdentityCipher] protects the envelope with a non-exportable Android
 * KeyStore key (`FR-AND-026 / 2`).
 *
 * Loading reports an identity only when the envelope, the cipher output and the text format all
 * validate. Unreadable or inconsistent stored content is reported as no identity so a new identity
 * can be generated, and a sender id is accepted only together with the pinned dedicated server
 * keys.
 *
 * Writing validates the identity before encrypting it, then builds a temporary sibling before
 * renaming it over the target, using an atomic move where the file system supports it, so a partial
 * file is never read back. The caller owns the directory and a missing directory is not created.
 * Reading and writing are blocking file I/O, so callers run them off the main thread. Not
 * thread-safe; access must stay on one thread or be externally synchronized.
 */
internal class FileInstallationIdentityStore(
    private val file: File,
    private val cipher: IdentityCipher,
) : InstallationIdentityStore {
    private val textFormat = InstallationIdentityTextFormat()

    override fun load(): InstallationIdentity? {
        if (!file.exists()) return null
        val content =
            try {
                file.readText(Charsets.UTF_8)
            } catch (_: IOException) {
                return null
            }
        val payload = parseEnvelope(content) ?: return null
        val plaintext =
            try {
                cipher.decrypt(payload)
            } catch (_: IdentityStorageException) {
                return null
            }
        return textFormat.parse(String(plaintext, Charsets.UTF_8))
    }

    override fun save(identity: InstallationIdentity) {
        val payload = cipher.encrypt(textFormat.format(identity).toByteArray(Charsets.UTF_8))
        val envelope = PAYLOAD_PREFIX + BASE64_ENCODER.encodeToString(payload) + NEWLINE
        val target = file.absoluteFile
        val directory =
            target.parentFile
                ?: throw IOException("Could not resolve the installation identity directory")
        val temporary = File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
        try {
            temporary.writeText(envelope, Charsets.UTF_8)
            moveOver(temporary, target)
        } finally {
            if (temporary.exists()) temporary.delete()
        }
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

    private companion object {
        const val TEMPORARY_FILE_PREFIX = "installation-identity-"
        const val TEMPORARY_FILE_SUFFIX = ".tmp"
        const val PAYLOAD_PREFIX = "payload="
        const val NEWLINE = "\n"
        const val EMPTY_LINE = ""
    }
}

/**
 * The text representation that [FileInstallationIdentityStore] reads and writes (`FR-DED-022 / 1`,
 * `INT-NET-008 / 2`, `INT-NET-025 / 1`).
 *
 * Every value is one `key=base64` line in UTF-8 with canonical padded Base64. A sender id is
 * meaningful only together with both dedicated server keys, because the client learns its sender
 * id only after first contact has pinned them. Writing refuses an identity whose keys, sender id or
 * server keys cannot form a valid text representation, and reading refuses content that does not
 * form one.
 */
internal class InstallationIdentityTextFormat {
    fun format(identity: InstallationIdentity): String {
        validate(identity)
        val lines =
            mutableListOf(
                line(KEY_SIGNING_PRIVATE, identity.signingKeyPair.privateKey),
                line(KEY_SIGNING_PUBLIC, identity.signingKeyPair.publicKey),
                line(KEY_AGREEMENT_PRIVATE, identity.agreementKeyPair.privateKey),
                line(KEY_AGREEMENT_PUBLIC, identity.agreementKeyPair.publicKey),
            )
        identity.senderId?.let { senderId -> lines += line(KEY_SENDER_ID, senderId) }
        identity.dedicatedServerKeys?.let { keys ->
            lines += line(KEY_SERVER_X25519, keys.x25519PublicKey)
            lines += line(KEY_SERVER_ED25519, keys.ed25519PublicKey)
        }
        return lines.joinToString(NEWLINE)
    }

    private fun validate(identity: InstallationIdentity) {
        val signingPrivate = identity.signingKeyPair.privateKey
        val signingPublic = identity.signingKeyPair.publicKey
        val agreementPrivate = identity.agreementKeyPair.privateKey
        val agreementPublic = identity.agreementKeyPair.publicKey

        requireKeyLength(KEY_SIGNING_PRIVATE, signingPrivate)
        requireKeyLength(KEY_SIGNING_PUBLIC, signingPublic)
        requireKeyLength(KEY_AGREEMENT_PRIVATE, agreementPrivate)
        requireKeyLength(KEY_AGREEMENT_PUBLIC, agreementPublic)

        if (!Ed25519.publicKeyOf(signingPrivate).contentEquals(signingPublic)) {
            throw IllegalArgumentException(KEY_SIGNING_PUBLIC)
        }
        if (!X25519.publicKeyOf(agreementPrivate).contentEquals(agreementPublic)) {
            throw IllegalArgumentException(KEY_AGREEMENT_PUBLIC)
        }

        identity.senderId?.let { senderId ->
            if (senderId.size != SENDER_ID_BYTES) throw IllegalArgumentException(KEY_SENDER_ID)
        }

        val serverX25519 = identity.dedicatedServerKeys?.x25519PublicKey
        val serverEd25519 = identity.dedicatedServerKeys?.ed25519PublicKey
        when {
            serverX25519 == null && serverEd25519 != null -> throw IllegalArgumentException(KEY_SERVER_X25519)
            serverX25519 != null && serverEd25519 == null -> throw IllegalArgumentException(KEY_SERVER_ED25519)
            serverX25519 != null && serverEd25519 != null -> {
                requireKeyLength(KEY_SERVER_X25519, serverX25519)
                requireKeyLength(KEY_SERVER_ED25519, serverEd25519)
            }
        }

        if (identity.senderId != null && serverX25519 == null) {
            throw IllegalArgumentException(KEY_SENDER_ID)
        }
    }

    private fun requireKeyLength(
        field: String,
        value: ByteArray,
    ) {
        if (value.size != KEY_BYTES) throw IllegalArgumentException(field)
    }

    fun parse(content: String): InstallationIdentity? {
        val values = parseLines(content) ?: return null

        val signingPrivate = values.bytesOf(KEY_SIGNING_PRIVATE, KEY_BYTES) ?: return null
        val signingPublic = values.bytesOf(KEY_SIGNING_PUBLIC, KEY_BYTES) ?: return null
        val agreementPrivate = values.bytesOf(KEY_AGREEMENT_PRIVATE, KEY_BYTES) ?: return null
        val agreementPublic = values.bytesOf(KEY_AGREEMENT_PUBLIC, KEY_BYTES) ?: return null

        val senderId = values[KEY_SENDER_ID]
        if (senderId != null && senderId.size != SENDER_ID_BYTES) return null

        val serverX25519 = values[KEY_SERVER_X25519]
        val serverEd25519 = values[KEY_SERVER_ED25519]
        val dedicatedServerKeys =
            when {
                serverX25519 == null && serverEd25519 == null -> null
                serverX25519 == null || serverEd25519 == null -> return null
                serverX25519.size != KEY_BYTES || serverEd25519.size != KEY_BYTES -> return null
                else ->
                    DedicatedServerKeys(
                        x25519PublicKey = serverX25519,
                        ed25519PublicKey = serverEd25519,
                    )
            }

        if (!Ed25519.publicKeyOf(signingPrivate).contentEquals(signingPublic)) return null
        if (!X25519.publicKeyOf(agreementPrivate).contentEquals(agreementPublic)) return null

        if (senderId != null && dedicatedServerKeys == null) return null

        return InstallationIdentity(
            signingKeyPair = Ed25519KeyPair(privateKey = signingPrivate, publicKey = signingPublic),
            agreementKeyPair = X25519KeyPair(privateKey = agreementPrivate, publicKey = agreementPublic),
            senderId = senderId,
            dedicatedServerKeys = dedicatedServerKeys,
        )
    }

    private fun line(
        key: String,
        value: ByteArray,
    ): String = "$key$KEY_SEPARATOR${BASE64_ENCODER.encodeToString(value)}"

    private fun parseLines(content: String): Map<String, ByteArray>? {
        val lines =
            content.split(NEWLINE).let { split ->
                if (split.lastOrNull() == EMPTY_LINE) split.dropLast(1) else split
            }
        val values = LinkedHashMap<String, ByteArray>()
        for (line in lines) {
            val separator = line.indexOf(KEY_SEPARATOR)
            if (separator <= 0) return null
            val key = line.substring(0, separator)
            if (key !in KEYS) return null
            val value = decode(line.substring(separator + 1)) ?: return null
            if (values.put(key, value) != null) return null
        }
        return values
    }

    private fun Map<String, ByteArray>.bytesOf(
        key: String,
        size: Int,
    ): ByteArray? = this[key]?.takeIf { value -> value.size == size }

    private fun decode(value: String): ByteArray? = decodeBase64(value)

    private companion object {
        const val KEY_SEPARATOR = '='
        const val NEWLINE = "\n"
        const val EMPTY_LINE = ""

        const val KEY_BYTES = 32
        const val SENDER_ID_BYTES = 16

        const val KEY_SIGNING_PRIVATE = "signingPrivate"
        const val KEY_SIGNING_PUBLIC = "signingPublic"
        const val KEY_AGREEMENT_PRIVATE = "agreementPrivate"
        const val KEY_AGREEMENT_PUBLIC = "agreementPublic"
        const val KEY_SENDER_ID = "senderId"
        const val KEY_SERVER_X25519 = "serverX25519"
        const val KEY_SERVER_ED25519 = "serverEd25519"

        val KEYS =
            setOf(
                KEY_SIGNING_PRIVATE,
                KEY_SIGNING_PUBLIC,
                KEY_AGREEMENT_PRIVATE,
                KEY_AGREEMENT_PUBLIC,
                KEY_SENDER_ID,
                KEY_SERVER_X25519,
                KEY_SERVER_ED25519,
            )
    }
}
