package dev.homecloud.android.protocol

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

/**
 * Reports that a protocol message or frame cannot be encoded or decoded according to the v1 wire
 * contract. Messages name the failing problem and never carry field contents or credentials.
 */
internal class ProtocolEncodingException(
    message: String,
) : Exception(message)

/**
 * The deterministic CBOR subset used by the v1 wire contract (`INT-NET-005`, `INT-NET-047`,
 * `INT-NET-049`).
 *
 * The subset contains unsigned and negative integers, byte strings, text strings, null and arrays.
 * Maps, tags, floating-point values, booleans and indefinite lengths are not part of it.
 */
internal sealed interface CborValue {
    /** A non-negative integer that is encoded in its shortest head form. */
    data class UInt(
        val value: Long,
    ) : CborValue

    /** A negative integer that is encoded in its shortest head form. */
    data class NInt(
        val value: Long,
    ) : CborValue

    /** A definite-length byte string. */
    data class Bytes(
        val value: ByteArray,
    ) : CborValue

    /** A definite-length UTF-8 text string. */
    data class Text(
        val value: String,
    ) : CborValue

    /** The CBOR null value used for an absent optional position. */
    data object Null : CborValue

    /** A definite-length array of values. */
    data class Array(
        val items: List<CborValue>,
    ) : CborValue
}

/**
 * Writes the deterministic CBOR subset (`INT-NET-005`).
 *
 * Every head uses the shortest possible argument form, so equal values always produce equal bytes.
 * Values outside the range of their CBOR type are rejected instead of being wrapped.
 */
internal object CborEncoder {
    fun encode(value: CborValue): ByteArray {
        val output = ByteArrayOutputStream()
        write(value, output)
        return output.toByteArray()
    }

    private fun write(
        value: CborValue,
        output: ByteArrayOutputStream,
    ) {
        when (value) {
            is CborValue.UInt -> {
                if (value.value < 0) {
                    throw ProtocolEncodingException("CBOR unsigned integer is negative")
                }
                writeHead(MAJOR_UNSIGNED, value.value, output)
            }
            is CborValue.NInt -> {
                if (value.value >= 0) {
                    throw ProtocolEncodingException("CBOR negative integer is not negative")
                }
                writeHead(MAJOR_NEGATIVE, -(value.value + 1), output)
            }
            is CborValue.Bytes -> {
                writeHead(MAJOR_BYTES, value.value.size.toLong(), output)
                output.write(value.value)
            }
            is CborValue.Text -> {
                val encoded = value.value.toByteArray(Charsets.UTF_8)
                writeHead(MAJOR_TEXT, encoded.size.toLong(), output)
                output.write(encoded)
            }
            is CborValue.Array -> {
                writeHead(MAJOR_ARRAY, value.items.size.toLong(), output)
                value.items.forEach { item -> write(item, output) }
            }
            CborValue.Null -> output.write(SIMPLE_NULL)
        }
    }

    private fun writeHead(
        majorType: Int,
        argument: Long,
        output: ByteArrayOutputStream,
    ) {
        val major = majorType shl MAJOR_TYPE_SHIFT
        when {
            argument < DIRECT_ARGUMENT_LIMIT -> output.write(major or argument.toInt())
            argument <= ONE_BYTE_MAX -> {
                output.write(major or ADDITIONAL_ONE_BYTE)
                writeBigEndian(argument, ONE_BYTE, output)
            }
            argument <= TWO_BYTE_MAX -> {
                output.write(major or ADDITIONAL_TWO_BYTES)
                writeBigEndian(argument, TWO_BYTES, output)
            }
            argument <= FOUR_BYTE_MAX -> {
                output.write(major or ADDITIONAL_FOUR_BYTES)
                writeBigEndian(argument, FOUR_BYTES, output)
            }
            else -> {
                output.write(major or ADDITIONAL_EIGHT_BYTES)
                writeBigEndian(argument, EIGHT_BYTES, output)
            }
        }
    }

    private fun writeBigEndian(
        argument: Long,
        byteCount: Int,
        output: ByteArrayOutputStream,
    ) {
        for (shift in (byteCount - 1) * BITS_PER_BYTE downTo 0 step BITS_PER_BYTE) {
            output.write(((argument ushr shift) and BYTE_MASK).toInt())
        }
    }
}

/**
 * Reads the deterministic CBOR subset (`INT-NET-005`, `INT-NET-047`, `INT-NET-049`).
 *
 * Only definite lengths and head forms with the shortest argument encoding are accepted. Input is
 * rejected when it is truncated, carries trailing bytes, or uses a CBOR type outside the subset.
 */
internal object CborDecoder {
    fun decode(bytes: ByteArray): CborValue {
        val decoder = Decoder(bytes)
        val value = decoder.readValue()
        if (decoder.hasRemaining()) {
            throw ProtocolEncodingException("trailing bytes after the CBOR value")
        }
        return value
    }

    private class Decoder(
        private val bytes: ByteArray,
    ) {
        private var position = 0

        fun hasRemaining(): Boolean = position < bytes.size

        fun readValue(): CborValue {
            val head = readHead()
            val majorType = (head ushr MAJOR_TYPE_SHIFT) and MAJOR_TYPE_MASK
            val additionalInformation = head and ADDITIONAL_MASK
            if (majorType == MAJOR_SIMPLE) {
                return readSimple(additionalInformation)
            }
            val argument = readArgument(additionalInformation)
            return when (majorType) {
                MAJOR_UNSIGNED -> readUnsignedValue(argument)
                MAJOR_NEGATIVE -> readNegativeValue(argument)
                MAJOR_BYTES -> CborValue.Bytes(readByteString(argument))
                MAJOR_TEXT -> CborValue.Text(readText(argument))
                MAJOR_ARRAY -> CborValue.Array(readArray(argument))
                MAJOR_MAP -> throw ProtocolEncodingException("CBOR maps are not supported")
                MAJOR_TAG -> throw ProtocolEncodingException("CBOR tags are not supported")
                else -> throw ProtocolEncodingException("unsupported CBOR major type $majorType")
            }
        }

        private fun readHead(): Int {
            if (position >= bytes.size) {
                throw ProtocolEncodingException("truncated CBOR input")
            }
            return bytes[position++].toInt() and BYTE_MASK_INT
        }

        private fun readArgument(additionalInformation: Int): Long =
            when (additionalInformation) {
                in 0..DIRECT_ARGUMENT_MAX -> additionalInformation.toLong()
                ADDITIONAL_ONE_BYTE ->
                    readUnsigned(ONE_BYTE).also { argument ->
                        if (argument < DIRECT_ARGUMENT_LIMIT) {
                            throw nonMinimalArgument()
                        }
                    }
                ADDITIONAL_TWO_BYTES ->
                    readUnsigned(TWO_BYTES).also { argument ->
                        if (argument <= ONE_BYTE_MAX) {
                            throw nonMinimalArgument()
                        }
                    }
                ADDITIONAL_FOUR_BYTES ->
                    readUnsigned(FOUR_BYTES).also { argument ->
                        if (argument <= TWO_BYTE_MAX) {
                            throw nonMinimalArgument()
                        }
                    }
                ADDITIONAL_EIGHT_BYTES ->
                    readUnsigned(EIGHT_BYTES).also { argument ->
                        if (argument in 0..FOUR_BYTE_MAX) {
                            throw nonMinimalArgument()
                        }
                    }
                ADDITIONAL_INDEFINITE ->
                    throw ProtocolEncodingException("indefinite-length CBOR values are not supported")
                else -> throw ProtocolEncodingException("reserved CBOR additional information")
            }

        private fun readUnsignedValue(argument: Long): CborValue {
            if (argument < 0) {
                throw ProtocolEncodingException("CBOR integer exceeds the supported range")
            }
            return CborValue.UInt(argument)
        }

        private fun readNegativeValue(argument: Long): CborValue {
            if (argument < 0) {
                throw ProtocolEncodingException("CBOR integer exceeds the supported range")
            }
            return CborValue.NInt(-1 - argument)
        }

        private fun readByteString(argument: Long): ByteArray {
            val length = checkedLength(argument)
            requireAvailable(length)
            val start = position
            position += length
            return bytes.copyOfRange(start, position)
        }

        private fun readText(argument: Long): String {
            val length = checkedLength(argument)
            requireAvailable(length)
            val start = position
            position += length
            return try {
                Charsets.UTF_8
                    .newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(bytes, start, length))
                    .toString()
            } catch (_: CharacterCodingException) {
                throw ProtocolEncodingException("CBOR text is not valid UTF-8")
            }
        }

        private fun readArray(argument: Long): List<CborValue> {
            val length = checkedLength(argument)
            if (length > remainingBytes()) {
                throw ProtocolEncodingException("truncated CBOR input")
            }
            return List(length) { readValue() }
        }

        private fun readSimple(additionalInformation: Int): CborValue =
            when (additionalInformation) {
                ADDITIONAL_NULL -> CborValue.Null
                ADDITIONAL_FALSE, ADDITIONAL_TRUE ->
                    throw ProtocolEncodingException("CBOR booleans are not supported")
                ADDITIONAL_HALF_FLOAT, ADDITIONAL_FLOAT, ADDITIONAL_DOUBLE ->
                    throw ProtocolEncodingException("CBOR floating-point values are not supported")
                else -> throw ProtocolEncodingException("unsupported CBOR simple value")
            }

        private fun readUnsigned(byteCount: Int): Long {
            requireAvailable(byteCount)
            var value = 0L
            repeat(byteCount) {
                value = (value shl BITS_PER_BYTE) or (bytes[position++].toLong() and BYTE_MASK)
            }
            return value
        }

        private fun checkedLength(argument: Long): Int {
            if (argument < 0 || argument > MAX_LENGTH) {
                throw ProtocolEncodingException("CBOR length exceeds the supported range")
            }
            return argument.toInt()
        }

        private fun requireAvailable(length: Int) {
            if (length > remainingBytes()) {
                throw ProtocolEncodingException("truncated CBOR input")
            }
        }

        private fun remainingBytes(): Int = bytes.size - position

        private fun nonMinimalArgument(): ProtocolEncodingException = ProtocolEncodingException("non-minimal CBOR head encoding")
    }
}

private const val MAJOR_UNSIGNED = 0
private const val MAJOR_NEGATIVE = 1
private const val MAJOR_BYTES = 2
private const val MAJOR_TEXT = 3
private const val MAJOR_ARRAY = 4
private const val MAJOR_MAP = 5
private const val MAJOR_TAG = 6
private const val MAJOR_SIMPLE = 7
private const val MAJOR_TYPE_SHIFT = 5
private const val MAJOR_TYPE_MASK = 0x07
private const val ADDITIONAL_MASK = 0x1F
private const val ADDITIONAL_ONE_BYTE = 24
private const val ADDITIONAL_TWO_BYTES = 25
private const val ADDITIONAL_FOUR_BYTES = 26
private const val ADDITIONAL_EIGHT_BYTES = 27
private const val ADDITIONAL_INDEFINITE = 31
private const val ADDITIONAL_FALSE = 20
private const val ADDITIONAL_TRUE = 21
private const val ADDITIONAL_NULL = 22
private const val ADDITIONAL_HALF_FLOAT = 25
private const val ADDITIONAL_FLOAT = 26
private const val ADDITIONAL_DOUBLE = 27
private const val DIRECT_ARGUMENT_LIMIT = 24L
private const val DIRECT_ARGUMENT_MAX = 23
private const val ONE_BYTE_MAX = 0xFFL
private const val TWO_BYTE_MAX = 0xFFFFL
private const val FOUR_BYTE_MAX = 0xFFFF_FFFFL
private const val ONE_BYTE = 1
private const val TWO_BYTES = 2
private const val FOUR_BYTES = 4
private const val EIGHT_BYTES = 8
private const val BITS_PER_BYTE = 8
private const val BYTE_MASK = 0xFFL
private const val BYTE_MASK_INT = 0xFF
private const val MAX_LENGTH = 2_147_483_647L
private const val SIMPLE_NULL = 0xF6
