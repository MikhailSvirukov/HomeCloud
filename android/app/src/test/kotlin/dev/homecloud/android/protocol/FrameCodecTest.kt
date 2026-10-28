package dev.homecloud.android.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream

class FrameCodecTest {
    @Test
    fun encodePrefixesPayloadWithBigEndianLength() {
        assertArrayEquals(EMPTY_FRAME, FrameCodec.encode(ByteArray(0), MAX_PAYLOAD_BYTES))
        assertArrayEquals(SINGLE_BYTE_FRAME, FrameCodec.encode(byteArrayOf(SINGLE_BYTE), MAX_PAYLOAD_BYTES))
        assertArrayEquals(LONG_PAYLOAD_FRAME_PREFIX + LONG_PAYLOAD, FrameCodec.encode(LONG_PAYLOAD, MAX_PAYLOAD_BYTES))
    }

    @Test
    fun decodeRoundTripsEncodedPayload() {
        val frame = FrameCodec.encode(LONG_PAYLOAD, MAX_PAYLOAD_BYTES)

        assertArrayEquals(LONG_PAYLOAD, FrameCodec.decode(frame, MAX_PAYLOAD_BYTES))
    }

    @Test
    fun payloadExactlyAtLimitRoundTripsOnEncodeAndDecode() {
        val payload = ByteArray(SMALL_LIMIT_BYTES) { index -> index.toByte() }

        val frame = FrameCodec.encode(payload, SMALL_LIMIT_BYTES)

        assertArrayEquals(payload, FrameCodec.decode(frame, SMALL_LIMIT_BYTES))
    }

    @Test
    fun encodeRejectsPayloadAboveLimit() {
        assertThrows(ProtocolEncodingException::class.java) {
            FrameCodec.encode(byteArrayOf(1, 2, 3, 4, 5), SMALL_LIMIT_BYTES)
        }
    }

    @Test
    fun decodeRejectsPayloadAboveLimit() {
        val frame = frame(byteArrayOf(1, 2, 3, 4, 5))

        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.decode(frame, SMALL_LIMIT_BYTES) }
    }

    @Test
    fun decodeRejectsDeclaredLengthThatDiffersFromPayloadPresent() {
        val declaredTooLong = frame(byteArrayOf(1, 2), declaredLength = 3)
        val declaredTooShort = frame(byteArrayOf(1, 2, 3), declaredLength = 2)

        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.decode(declaredTooLong, MAX_PAYLOAD_BYTES) }
        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.decode(declaredTooShort, MAX_PAYLOAD_BYTES) }
    }

    @Test
    fun decodeRejectsFrameShorterThanLengthPrefix() {
        val truncatedFrame = byteArrayOf(0, 0, 0)

        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.decode(truncatedFrame, MAX_PAYLOAD_BYTES) }
    }

    @Test
    fun readFromReadsSingleFrameAndLeavesFollowingFrameInStream() {
        val firstPayload = byteArrayOf(1, 2, 3)
        val secondFrame = FrameCodec.encode(byteArrayOf(4, 5), MAX_PAYLOAD_BYTES)
        val input = ByteArrayInputStream(FrameCodec.encode(firstPayload, MAX_PAYLOAD_BYTES) + secondFrame)

        assertArrayEquals(firstPayload, FrameCodec.readFrom(input, MAX_PAYLOAD_BYTES))
        assertEquals(secondFrame.size, input.available())
        assertArrayEquals(secondFrame, input.readBytes())
    }

    @Test
    fun readFromRejectsOverLimitLengthBeforeConsumingPayload() {
        val payload = byteArrayOf(1, 2, 3, 4, 5)
        val input = ByteArrayInputStream(frame(payload))

        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.readFrom(input, SMALL_LIMIT_BYTES) }

        assertEquals(payload.size, input.available())
        assertArrayEquals(payload, input.readBytes())
    }

    @Test
    fun frameCodecRejectsNonPositiveLimit() {
        assertThrows(ProtocolEncodingException::class.java) { FrameCodec.encode(ByteArray(0), ZERO_LIMIT_BYTES) }
        assertThrows(ProtocolEncodingException::class.java) {
            FrameCodec.decode(EMPTY_FRAME, ZERO_LIMIT_BYTES)
        }
        assertThrows(ProtocolEncodingException::class.java) {
            FrameCodec.readFrom(ByteArrayInputStream(EMPTY_FRAME), NEGATIVE_LIMIT_BYTES)
        }
    }

    private fun frame(
        payload: ByteArray,
        declaredLength: Int = payload.size,
    ): ByteArray = lengthPrefix(declaredLength) + payload

    private fun lengthPrefix(length: Int): ByteArray =
        byteArrayOf(
            (length ushr (BITS_PER_BYTE * THREE_BYTES)).toByte(),
            (length ushr (BITS_PER_BYTE * TWO_BYTES)).toByte(),
            (length ushr BITS_PER_BYTE).toByte(),
            length.toByte(),
        )

    private companion object {
        const val MAX_PAYLOAD_BYTES = 1_024
        const val SMALL_LIMIT_BYTES = 4
        const val ZERO_LIMIT_BYTES = 0
        const val NEGATIVE_LIMIT_BYTES = -1
        const val BITS_PER_BYTE = 8
        const val TWO_BYTES = 2
        const val THREE_BYTES = 3
        const val SINGLE_BYTE: Byte = 42

        val EMPTY_FRAME = byteArrayOf(0, 0, 0, 0)
        val SINGLE_BYTE_FRAME = byteArrayOf(0, 0, 0, 1, SINGLE_BYTE)
        val LONG_PAYLOAD_FRAME_PREFIX = byteArrayOf(0, 0, 1, 0)
        val LONG_PAYLOAD = ByteArray(256) { index -> index.toByte() }
    }
}
