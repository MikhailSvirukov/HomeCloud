package dev.homecloud.android.testsupport

/**
 * Decodes hexadecimal test vectors into the bytes they encode.
 */
internal object Hex {
    /**
     * Decodes pairs of hexadecimal digits, in either letter case, into one byte each.
     */
    fun decode(value: String): ByteArray =
        ByteArray(value.length / HEX_DIGITS_PER_BYTE) { index ->
            val offset = index * HEX_DIGITS_PER_BYTE
            value.substring(offset, offset + HEX_DIGITS_PER_BYTE).toInt(HEX_RADIX).toByte()
        }

    private const val HEX_DIGITS_PER_BYTE = 2
    private const val HEX_RADIX = 16
}
