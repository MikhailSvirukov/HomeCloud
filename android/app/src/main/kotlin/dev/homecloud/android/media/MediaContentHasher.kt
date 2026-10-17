package dev.homecloud.android.media

import java.io.InputStream
import java.security.MessageDigest

/**
 * Streams media bytes through SHA-256 so one version can be identified without holding the file in
 * memory.
 *
 * The digest covers exactly the bytes read from the caller's stream, which is the original file
 * content before compression or encryption. Reading uses one fixed-size block at a time, and the
 * caller keeps ownership of the stream.
 */
internal object MediaContentHasher {
    /** Streams [input] through SHA-256 and produces the lowercase hex digest, leaving the stream open. */
    fun sha256Hex(input: InputStream): String {
        val digest = MessageDigest.getInstance(DIGEST_ALGORITHM)
        val buffer = ByteArray(BUFFER_SIZE_BYTES)
        var read = input.read(buffer)
        while (read != END_OF_STREAM) {
            digest.update(buffer, 0, read)
            read = input.read(buffer)
        }
        return digest.digest().toLowercaseHex()
    }

    private fun ByteArray.toLowercaseHex(): String {
        val hex = StringBuilder(size * HEX_CHARS_PER_BYTE)
        for (byte in this) {
            val value = byte.toInt() and BYTE_MASK
            hex.append(HEX_DIGITS[value ushr NIBBLE_BITS])
            hex.append(HEX_DIGITS[value and NIBBLE_MASK])
        }
        return hex.toString()
    }

    private const val DIGEST_ALGORITHM = "SHA-256"
    private const val BUFFER_SIZE_BYTES = 8 * 1024
    private const val END_OF_STREAM = -1
    private const val HEX_CHARS_PER_BYTE = 2
    private const val BYTE_MASK = 0xFF
    private const val NIBBLE_BITS = 4
    private const val NIBBLE_MASK = 0x0F
    private val HEX_DIGITS = "0123456789abcdef".toCharArray()
}
