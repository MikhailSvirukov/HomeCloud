package dev.homecloud.android.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class CborCodecTest {
    @Test
    fun encodeUsesShortestHeadForIntegers() {
        val vectors =
            listOf(
                CborValue.UInt(0) to HEX_UNSIGNED_ZERO,
                CborValue.UInt(23) to HEX_UNSIGNED_TWENTY_THREE,
                CborValue.UInt(24) to HEX_UNSIGNED_TWENTY_FOUR,
                CborValue.UInt(1000) to HEX_UNSIGNED_THOUSAND,
                CborValue.UInt(1_000_000) to HEX_UNSIGNED_MILLION,
                CborValue.NInt(-1) to HEX_NEGATIVE_ONE,
                CborValue.NInt(-100) to HEX_NEGATIVE_HUNDRED,
            )

        vectors.forEach { (value, expected) ->
            assertEquals(expected, hexOf(CborEncoder.encode(value)))
        }
    }

    @Test
    fun encodeWritesTextStringsWithUtf8Length() {
        assertEquals(HEX_TEXT_A, hexOf(CborEncoder.encode(CborValue.Text(TEXT_A))))
        assertEquals(HEX_TEXT_IETF, hexOf(CborEncoder.encode(CborValue.Text(TEXT_IETF))))
    }

    @Test
    fun encodeWritesByteStringContent() {
        val expected = hexOf(CborEncoder.encode(CborValue.Bytes(BYTE_STRING)))

        assertEquals(HEX_BYTE_STRING, expected)
    }

    @Test
    fun encodeWritesDefiniteLengthArrays() {
        assertEquals(HEX_EMPTY_ARRAY, hexOf(CborEncoder.encode(CborValue.Array(emptyList()))))
        assertEquals(
            HEX_SMALL_ARRAY,
            hexOf(CborEncoder.encode(CborValue.Array(listOf(CborValue.UInt(1), CborValue.UInt(2), CborValue.UInt(3))))),
        )
        assertEquals(
            HEX_NESTED_ARRAY,
            hexOf(
                CborEncoder.encode(
                    CborValue.Array(
                        listOf(
                            CborValue.UInt(1),
                            CborValue.Array(listOf(CborValue.UInt(2), CborValue.UInt(3))),
                        ),
                    ),
                ),
            ),
        )
    }

    @Test
    fun encodeWritesNullAsSimpleValue() {
        assertEquals(HEX_NULL, hexOf(CborEncoder.encode(CborValue.Null)))
    }

    @Test
    fun decodeRoundTripsScalarValues() {
        val values =
            listOf(
                CborValue.UInt(24),
                CborValue.UInt(1_000_000),
                CborValue.NInt(-100),
                CborValue.Bytes(BYTE_STRING),
                CborValue.Text(TEXT_IETF),
                CborValue.Null,
            )

        values.forEach { value ->
            assertCborEquals(value, CborDecoder.decode(CborEncoder.encode(value)))
        }
    }

    @Test
    fun decodeRoundTripsNestedArrays() {
        val value =
            CborValue.Array(
                listOf(
                    CborValue.UInt(1),
                    CborValue.Array(listOf(CborValue.Text(TEXT_A), CborValue.Null)),
                ),
            )

        assertCborEquals(value, CborDecoder.decode(CborEncoder.encode(value)))
    }

    @Test
    fun decodeRejectsNonMinimalIntegerHead() {
        assertRejected(HEX_NON_MINIMAL_INTEGER)
    }

    @Test
    fun decodeRejectsTrailingBytes() {
        assertRejected(HEX_VALUE_WITH_TRAILING_BYTE)
    }

    @Test
    fun decodeRejectsIndefiniteLengthArray() {
        assertRejected(HEX_INDEFINITE_ARRAY)
    }

    @Test
    fun decodeRejectsMap() {
        assertRejected(HEX_MAP)
    }

    @Test
    fun decodeRejectsBoolean() {
        assertRejected(HEX_BOOLEAN_TRUE)
    }

    @Test
    fun decodeRejectsTruncatedByteString() {
        assertRejected(HEX_TRUNCATED_BYTE_STRING)
    }

    @Test
    fun decodeRejectsMalformedUtf8Text() {
        assertRejected(HEX_MALFORMED_UTF8)
    }

    @Test
    fun decodeRejectsTag() {
        assertRejected(HEX_TAG)
    }

    @Test
    fun decodeRejectsHalfPrecisionFloat() {
        assertRejected(HEX_HALF_FLOAT)
    }

    @Test
    fun decodeRejectsSinglePrecisionFloat() {
        assertRejected(HEX_SINGLE_FLOAT)
    }

    @Test
    fun decodeRejectsDoublePrecisionFloat() {
        assertRejected(HEX_DOUBLE_FLOAT)
    }

    @Test
    fun decodeRejectsFalseSimpleValue() {
        assertRejected(HEX_SIMPLE_FALSE)
    }

    @Test
    fun decodeRejectsUnassignedSimpleValue() {
        assertRejected(HEX_SIMPLE_UNASSIGNED)
    }

    @Test
    fun decodeRejectsTwoByteSimpleValue() {
        assertRejected(HEX_SIMPLE_TWO_BYTE)
    }

    @Test
    fun decodeRejectsIndefiniteLengthByteString() {
        assertRejected(HEX_INDEFINITE_BYTE_STRING)
    }

    @Test
    fun decodeRejectsIndefiniteLengthTextString() {
        assertRejected(HEX_INDEFINITE_TEXT_STRING)
    }

    @Test
    fun decodeRejectsNonMinimalTwoByteHead() {
        assertRejected(HEX_NON_MINIMAL_TWO_BYTE_HEAD)
    }

    @Test
    fun decodeRejectsNonMinimalFourByteHead() {
        assertRejected(HEX_NON_MINIMAL_FOUR_BYTE_HEAD)
    }

    @Test
    fun decodeRejectsNonMinimalEightByteHead() {
        assertRejected(HEX_NON_MINIMAL_EIGHT_BYTE_HEAD)
    }

    @Test
    fun decodeRejectsTruncatedArray() {
        assertRejected(HEX_TRUNCATED_ARRAY)
    }

    @Test
    fun decodeAcceptsUnsignedValueAtLongMax() {
        assertEquals(CborValue.UInt(LONG_MAX), CborDecoder.decode(bytesOf(HEX_UNSIGNED_LONG_MAX)))
    }

    @Test
    fun decodeRejectsUnsignedValueAboveLongMax() {
        assertRejected(HEX_UNSIGNED_ABOVE_LONG_MAX)
    }

    @Test
    fun decodeAcceptsNestingAtSixtyFourLevels() {
        var nested: CborValue = CborValue.UInt(0)
        repeat(NESTING_LEVELS_AT_LIMIT) {
            nested = CborValue.Array(listOf(nested))
        }

        assertCborEquals(nested, CborDecoder.decode(CborEncoder.encode(nested)))
    }

    @Test
    fun decodeRejectsNestingDeeperThanSixtyFourLevels() {
        var nested: CborValue = CborValue.UInt(0)
        repeat(NESTING_LEVELS_ABOVE_LIMIT) {
            nested = CborValue.Array(listOf(nested))
        }

        assertThrows(ProtocolEncodingException::class.java) { CborDecoder.decode(CborEncoder.encode(nested)) }
    }

    @Test
    fun encodeRejectsNegativeUnsignedValue() {
        assertThrows(ProtocolEncodingException::class.java) { CborEncoder.encode(CborValue.UInt(-1)) }
    }

    @Test
    fun encodeRejectsNegativeIntegerAtZero() {
        assertThrows(ProtocolEncodingException::class.java) { CborEncoder.encode(CborValue.NInt(0)) }
    }

    private fun assertRejected(hex: String) {
        assertThrows(ProtocolEncodingException::class.java) { CborDecoder.decode(bytesOf(hex)) }
    }

    private fun assertCborEquals(
        expected: CborValue,
        actual: CborValue,
    ) {
        when (expected) {
            is CborValue.UInt -> assertEquals(expected, actual)
            is CborValue.NInt -> assertEquals(expected, actual)
            is CborValue.Text -> assertEquals(expected, actual)
            CborValue.Null -> assertEquals(expected, actual)
            is CborValue.Bytes -> {
                assertTrue(actual is CborValue.Bytes)
                assertArrayEquals(expected.value, (actual as CborValue.Bytes).value)
            }
            is CborValue.Array -> {
                assertTrue(actual is CborValue.Array)
                val actualItems = (actual as CborValue.Array).items
                assertEquals(expected.items.size, actualItems.size)
                expected.items.forEachIndexed { index, item ->
                    assertCborEquals(item, actualItems[index])
                }
            }
        }
    }

    private fun hexOf(bytes: ByteArray): String = bytes.joinToString(separator = "") { byte -> "%02X".format(byte) }

    private fun bytesOf(hex: String): ByteArray =
        hex.chunked(HEX_DIGITS_PER_BYTE).map { chunk -> chunk.toInt(HEX_RADIX).toByte() }.toByteArray()

    private companion object {
        const val TEXT_A = "a"
        const val TEXT_IETF = "IETF"
        const val HEX_DIGITS_PER_BYTE = 2
        const val HEX_RADIX = 16

        val BYTE_STRING = byteArrayOf(1, 2, 3, 4)

        const val HEX_UNSIGNED_ZERO = "00"
        const val HEX_UNSIGNED_TWENTY_THREE = "17"
        const val HEX_UNSIGNED_TWENTY_FOUR = "1818"
        const val HEX_UNSIGNED_THOUSAND = "1903E8"
        const val HEX_UNSIGNED_MILLION = "1A000F4240"
        const val HEX_NEGATIVE_ONE = "20"
        const val HEX_NEGATIVE_HUNDRED = "3863"
        const val HEX_TEXT_A = "6161"
        const val HEX_TEXT_IETF = "6449455446"
        const val HEX_BYTE_STRING = "4401020304"
        const val HEX_EMPTY_ARRAY = "80"
        const val HEX_SMALL_ARRAY = "83010203"
        const val HEX_NESTED_ARRAY = "8201820203"
        const val HEX_NULL = "F6"

        const val HEX_NON_MINIMAL_INTEGER = "1817"
        const val HEX_VALUE_WITH_TRAILING_BYTE = "0100"
        const val HEX_INDEFINITE_ARRAY = "9F"
        const val HEX_MAP = "A0"
        const val HEX_BOOLEAN_TRUE = "F5"
        const val HEX_TRUNCATED_BYTE_STRING = "430102"
        const val HEX_MALFORMED_UTF8 = "61FF"

        const val HEX_TAG = "C100"
        const val HEX_HALF_FLOAT = "F90000"
        const val HEX_SINGLE_FLOAT = "FA00000000"
        const val HEX_DOUBLE_FLOAT = "FB0000000000000000"
        const val HEX_SIMPLE_FALSE = "F7"
        const val HEX_SIMPLE_UNASSIGNED = "F0"
        const val HEX_SIMPLE_TWO_BYTE = "F820"
        const val HEX_INDEFINITE_BYTE_STRING = "5F4100FF"
        const val HEX_INDEFINITE_TEXT_STRING = "7F6161FF"
        const val HEX_NON_MINIMAL_TWO_BYTE_HEAD = "190017"
        const val HEX_NON_MINIMAL_FOUR_BYTE_HEAD = "1A00000017"
        const val HEX_NON_MINIMAL_EIGHT_BYTE_HEAD = "1B0000000000000017"
        const val HEX_TRUNCATED_ARRAY = "8201"
        const val HEX_UNSIGNED_ABOVE_LONG_MAX = "1BFFFFFFFFFFFFFFFF"
        const val HEX_UNSIGNED_LONG_MAX = "1B7FFFFFFFFFFFFFFF"
        const val LONG_MAX = Long.MAX_VALUE
        const val NESTING_LEVELS_AT_LIMIT = 64
        const val NESTING_LEVELS_ABOVE_LIMIT = 65
    }
}
