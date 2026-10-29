package dev.homecloud.android.protocol

import java.io.InputStream

/**
 * Length-prefixed application framing for the v1 protocol (`INT-NET-002`, `INT-NET-003`,
 * `INT-NET-004`, `CFG-CL-011`).
 *
 * A frame is a four-byte big-endian unsigned payload length followed by exactly that many payload
 * bytes. The configured payload limit is applied to encoding, to decoding a complete frame and to
 * reading a single frame from a stream, and it is checked before the payload is allocated.
 */
internal object FrameCodec {
    fun encode(
        payload: ByteArray,
        maxPayloadBytes: Int,
    ): ByteArray {
        requirePositiveLimit(maxPayloadBytes)
        if (payload.size > maxPayloadBytes) {
            throw ProtocolEncodingException("payload exceeds the maximum frame size")
        }
        val frame = ByteArray(LENGTH_PREFIX_BYTES + payload.size)
        writeLength(payload.size, frame)
        payload.copyInto(frame, LENGTH_PREFIX_BYTES)
        return frame
    }

    fun decode(
        frame: ByteArray,
        maxPayloadBytes: Int,
    ): ByteArray {
        requirePositiveLimit(maxPayloadBytes)
        if (frame.size < LENGTH_PREFIX_BYTES) {
            throw ProtocolEncodingException("frame is shorter than the length prefix")
        }
        val declaredLength = readLength(frame)
        val presentLength = (frame.size - LENGTH_PREFIX_BYTES).toLong()
        if (declaredLength != presentLength) {
            throw ProtocolEncodingException("frame length does not match the declared payload length")
        }
        if (declaredLength > maxPayloadBytes.toLong()) {
            throw ProtocolEncodingException("payload exceeds the maximum frame size")
        }
        return frame.copyOfRange(LENGTH_PREFIX_BYTES, frame.size)
    }

    fun readFrom(
        input: InputStream,
        maxPayloadBytes: Int,
    ): ByteArray {
        requirePositiveLimit(maxPayloadBytes)
        val prefix = ByteArray(LENGTH_PREFIX_BYTES)
        var prefixBytes = 0
        while (prefixBytes < LENGTH_PREFIX_BYTES) {
            val count = input.read(prefix, prefixBytes, LENGTH_PREFIX_BYTES - prefixBytes)
            if (count < 0) {
                throw ProtocolEncodingException("frame length prefix is truncated")
            }
            prefixBytes += count
        }
        val declaredLength = readLength(prefix)
        if (declaredLength > maxPayloadBytes.toLong()) {
            throw ProtocolEncodingException("payload exceeds the maximum frame size")
        }
        val payload = ByteArray(declaredLength.toInt())
        var payloadBytes = 0
        while (payloadBytes < payload.size) {
            val count = input.read(payload, payloadBytes, payload.size - payloadBytes)
            if (count < 0) {
                throw ProtocolEncodingException("frame payload is truncated")
            }
            payloadBytes += count
        }
        return payload
    }

    private fun requirePositiveLimit(maxPayloadBytes: Int) {
        if (maxPayloadBytes <= 0) {
            throw ProtocolEncodingException("maximum payload size must be positive")
        }
    }

    private fun writeLength(
        length: Int,
        destination: ByteArray,
    ) {
        destination[0] = (length ushr (BITS_PER_BYTE * 3)).toByte()
        destination[1] = (length ushr (BITS_PER_BYTE * 2)).toByte()
        destination[2] = (length ushr BITS_PER_BYTE).toByte()
        destination[3] = length.toByte()
    }

    private fun readLength(frame: ByteArray): Long {
        var length = 0L
        for (index in 0 until LENGTH_PREFIX_BYTES) {
            length = (length shl BITS_PER_BYTE) or (frame[index].toLong() and BYTE_MASK)
        }
        return length
    }
}

private const val LENGTH_PREFIX_BYTES = 4
private const val BITS_PER_BYTE = 8
private const val BYTE_MASK = 0xFFL
