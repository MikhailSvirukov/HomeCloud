package dev.homecloud.android.protocol

import org.junit.Assert.assertEquals
import org.junit.Test

class CborDecodePrefixTest {
    @Test
    fun decodePrefixReadsOneValueFromStartAndReportsConsumedBytes() {
        val value = CborValue.Array(listOf(CborValue.UInt(7), CborValue.Text("node")))
        val encoded = CborEncoder.encode(value)

        val decoded = CborDecoder.decodePrefix(encoded)

        assertEquals(value, decoded.value)
        assertEquals(encoded.size, decoded.consumedBytes)
    }

    @Test
    fun decodePrefixStartsAtOffsetAndIgnoresTrailingBytes() {
        val encoded = CborEncoder.encode(CborValue.UInt(1_000))
        val bytes = byteArrayOf(0x01, 0x02) + encoded + byteArrayOf(0x03)

        val decoded = CborDecoder.decodePrefix(bytes, OFFSET)

        assertEquals(CborValue.UInt(1_000), decoded.value)
        assertEquals(encoded.size, decoded.consumedBytes)
    }

    private companion object {
        const val OFFSET = 2
    }
}
