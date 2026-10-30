package dev.homecloud.android.protocol

import dev.homecloud.android.crypto.CryptoException
import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Ed25519KeyPair
import dev.homecloud.android.crypto.X25519
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class FrameBodyCodecTest {
    @Test
    fun signedBytesStartsWithBigEndianBodyLengthPrefix() {
        val headerBytes = byteArrayOf(0x01, 0x02, 0x03)
        val payload = byteArrayOf(0x0A, 0x0B)

        val signed = FrameBodyCodec.signedBytes(headerBytes, payload)

        val expectedPrefix = byteArrayOf(0x00, 0x00, 0x00, 0x45)
        assertArrayEquals(expectedPrefix + headerBytes + payload, signed)
    }

    @Test
    fun encodeAndDecodeRoundTripsHeaderPayloadAndSignature() {
        val keyPair = Ed25519.generate()
        val header = ordinaryHeader()
        val frame = encodeFrame(header, PAYLOAD, keyPair)

        val decoded = decodeFrame(frame, keyPair)

        assertBytes(SENDER_ID, decoded.header.senderId)
        assertArrayEquals(REQUEST_ID, decoded.header.requestId)
        assertEquals(FRAME_NUMBER, decoded.header.frameNumber)
        assertBytes(UPLOAD_ID, decoded.header.uploadId)
        assertEquals(RESPONDER_LABEL, decoded.header.responderLabel)
        assertNull(decoded.header.bootstrapSigningPublicKey)
        assertEquals(SIGNATURE_BYTES, decoded.signature.size)
        assertArrayEquals(PAYLOAD, decoded.payload)
    }

    @Test
    fun decodeRejectsTamperedSignature() {
        val keyPair = Ed25519.generate()
        val header = ordinaryHeader()
        val frame = encodeFrame(header, PAYLOAD, keyPair)
        val signatureStart = LENGTH_PREFIX_BYTES + externalHeaderSize(header)
        val tampered = frame.copyOf()
        tampered[signatureStart] = flipLowestBit(tampered[signatureStart])

        assertThrows(ProtocolEncodingException::class.java) { decodeFrame(tampered, keyPair) }
    }

    @Test
    fun decodeRejectsTamperedPayload() {
        val keyPair = Ed25519.generate()
        val frame = encodeFrame(ordinaryHeader(), PAYLOAD, keyPair)
        val tampered = frame.copyOf()
        tampered[tampered.lastIndex] = flipLowestBit(tampered[tampered.lastIndex])

        assertThrows(ProtocolEncodingException::class.java) { decodeFrame(tampered, keyPair) }
    }

    @Test
    fun decodeRejectsTamperedHeader() {
        val keyPair = Ed25519.generate()
        val frame = encodeFrame(ordinaryHeader(), PAYLOAD, keyPair)
        val tampered = frame.copyOf()
        val senderIdContentOffset = LENGTH_PREFIX_BYTES + SENDER_ID_CONTENT_OFFSET
        tampered[senderIdContentOffset] = flipLowestBit(tampered[senderIdContentOffset])

        assertThrows(ProtocolEncodingException::class.java) { decodeFrame(tampered, keyPair) }
    }

    @Test
    fun decodeThrowsWhenVerifierReturnsFalse() {
        val frame = encodeFrame(ordinaryHeader(), PAYLOAD, Ed25519.generate())

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { _, _ -> false }
        }
    }

    @Test
    fun decodeVerifiesSignedBytesOverPrefixHeaderAndPayload() {
        val keyPair = Ed25519.generate()
        val header = ordinaryHeader()
        val frame = encodeFrame(header, PAYLOAD, keyPair)
        val headerBytes = ExternalFrameHeaderCodec.encode(header)
        var capturedSignedBytes = EMPTY
        var capturedSignature = EMPTY

        FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { signedBytes, signature ->
            capturedSignedBytes = signedBytes
            capturedSignature = signature
            Ed25519.verify(keyPair.publicKey, signedBytes, signature)
        }

        assertArrayEquals(FrameBodyCodec.signedBytes(headerBytes, PAYLOAD), capturedSignedBytes)
        assertEquals(SIGNATURE_BYTES, capturedSignature.size)
    }

    @Test
    fun encodeRejectsBodyAboveLimit() {
        val keyPair = Ed25519.generate()
        val header = ordinaryHeader()
        val maxFrameBytes = externalHeaderSize(header) + SIGNATURE_BYTES + PAYLOAD.size - 1

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.encode(
                header = header,
                payload = PAYLOAD,
                sign = { signedBytes -> Ed25519.sign(keyPair.privateKey, signedBytes) },
                maxFrameBytes = maxFrameBytes,
            )
        }
    }

    @Test
    fun decodeRejectsDeclaredLengthAboveLimit() {
        val declaredLength = MAX_FRAME_BYTES + 1
        val frame = lengthPrefix(declaredLength) + ByteArray(declaredLength)

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { _, _ -> false }
        }
    }

    @Test
    fun decodeRejectsDeclaredLengthThatDiffersFromBodyPresent() {
        val declaredTooLong = lengthPrefix(2) + byteArrayOf(1)
        val declaredTooShort = lengthPrefix(1) + byteArrayOf(1, 2)

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(declaredTooLong, MAX_FRAME_BYTES) { _, _ -> false }
        }
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(declaredTooShort, MAX_FRAME_BYTES) { _, _ -> false }
        }
    }

    @Test
    fun decodeRejectsFrameShorterThanLengthPrefix() {
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(byteArrayOf(0, 0, 0), MAX_FRAME_BYTES) { _, _ -> false }
        }
    }

    @Test
    fun frameBodyCodecRejectsNonPositiveLimit() {
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.encode(ordinaryHeader(), PAYLOAD, { EMPTY }, ZERO_LIMIT)
        }
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decode(byteArrayOf(0, 0, 0, 0), NEGATIVE_LIMIT) { _, _ -> false }
        }
    }

    @Test
    fun encryptedPayloadContainsEncapsulatedKeyAndCiphertext() {
        val recipient = X25519.generate()

        val payload = FrameBodyCodec.encryptedPayload(recipient.publicKey, PLAINTEXT)

        assertEquals(KEY_BYTES + PLAINTEXT.size + GCM_TAG_BYTES, payload.size)
    }

    @Test
    fun decryptedPayloadRecoversPlaintext() {
        val recipient = X25519.generate()
        val payload = FrameBodyCodec.encryptedPayload(recipient.publicKey, PLAINTEXT)

        val decrypted = FrameBodyCodec.decryptedPayload(recipient.privateKey, payload)

        assertArrayEquals(PLAINTEXT, decrypted)
    }

    @Test
    fun decryptedPayloadWithDifferentKeyThrowsCryptoException() {
        val recipient = X25519.generate()
        val otherRecipient = X25519.generate()
        val payload = FrameBodyCodec.encryptedPayload(recipient.publicKey, PLAINTEXT)

        assertThrows(CryptoException::class.java) {
            FrameBodyCodec.decryptedPayload(otherRecipient.privateKey, payload)
        }
    }

    @Test
    fun decryptedPayloadShorterThanEncapsulatedKeyAndTagIsRejected() {
        val shortPayload = ByteArray(KEY_BYTES + GCM_TAG_BYTES - 1)

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decryptedPayload(ByteArray(KEY_BYTES), shortPayload)
        }
    }

    @Test
    fun bootstrapPayloadRoundTripsKeysInX25519ThenEd25519Order() {
        val x25519PublicKey = ByteArray(KEY_BYTES) { X25519_KEY_FILL }
        val ed25519PublicKey = ByteArray(KEY_BYTES) { ED25519_KEY_FILL }

        val payload = FrameBodyCodec.bootstrapPayload(x25519PublicKey, ed25519PublicKey)
        val (decodedX25519Key, decodedEd25519Key) = FrameBodyCodec.bootstrapKeys(payload)

        assertArrayEquals(x25519PublicKey, decodedX25519Key)
        assertArrayEquals(ed25519PublicKey, decodedEd25519Key)
    }

    @Test
    fun bootstrapPayloadRejectsKeyThatIsNotThirtyTwoBytes() {
        val validKey = ByteArray(KEY_BYTES)

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.bootstrapPayload(ByteArray(KEY_BYTES - 1), validKey)
        }
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.bootstrapPayload(validKey, ByteArray(KEY_BYTES + 1))
        }
    }

    @Test
    fun bootstrapKeysRejectsMalformedArrays() {
        val key = CborValue.Bytes(ByteArray(KEY_BYTES))
        val threeElements = CborEncoder.encode(CborValue.Array(listOf(key, key, key)))
        val textElement = CborEncoder.encode(CborValue.Array(listOf(CborValue.Text("not a key"), key)))
        val shortKey = CborEncoder.encode(CborValue.Array(listOf(CborValue.Bytes(ByteArray(KEY_BYTES - 1)), key)))

        assertThrows(ProtocolEncodingException::class.java) { FrameBodyCodec.bootstrapKeys(threeElements) }
        assertThrows(ProtocolEncodingException::class.java) { FrameBodyCodec.bootstrapKeys(textElement) }
        assertThrows(ProtocolEncodingException::class.java) { FrameBodyCodec.bootstrapKeys(shortKey) }
    }

    @Test
    fun decodeBootstrapReturnsHeaderAndBothKeysFromValidFrame() {
        val x25519KeyPair = X25519.generate()
        val ed25519KeyPair = Ed25519.generate()
        val header = bootstrapHeader()
        val frame = bootstrapFrame(x25519KeyPair.publicKey, ed25519KeyPair)

        val bootstrap = FrameBodyCodec.decodeBootstrap(frame, MAX_FRAME_BYTES)

        assertBytes(SENDER_ID, bootstrap.header.senderId)
        assertArrayEquals(REQUEST_ID, bootstrap.header.requestId)
        assertEquals(FRAME_NUMBER, bootstrap.header.frameNumber)
        assertNull(bootstrap.header.uploadId)
        assertNull(bootstrap.header.responderLabel)
        assertNull(bootstrap.header.bootstrapSigningPublicKey)
        assertArrayEquals(x25519KeyPair.publicKey, bootstrap.x25519PublicKey)
        assertArrayEquals(ed25519KeyPair.publicKey, bootstrap.ed25519PublicKey)
    }

    @Test
    fun decodeBootstrapRejectsThePrimaryRegistrationHeaderShape() {
        val x25519KeyPair = X25519.generate()
        val ed25519KeyPair = Ed25519.generate()
        val frame =
            FrameBodyCodec.encode(
                header =
                    ExternalFrameHeader(
                        senderId = null,
                        requestId = REQUEST_ID,
                        frameNumber = FRAME_NUMBER,
                        uploadId = null,
                        responderLabel = null,
                        bootstrapSigningPublicKey = ed25519KeyPair.publicKey,
                    ),
                payload = FrameBodyCodec.bootstrapPayload(x25519KeyPair.publicKey, ed25519KeyPair.publicKey),
                sign = { signedBytes -> Ed25519.sign(ed25519KeyPair.privateKey, signedBytes) },
                maxFrameBytes = MAX_FRAME_BYTES,
            )

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decodeBootstrap(frame, MAX_FRAME_BYTES)
        }
    }

    @Test
    fun decodeBootstrapRejectsTamperedSignature() {
        val ed25519KeyPair = Ed25519.generate()
        val frame = bootstrapFrame(X25519.generate().publicKey, ed25519KeyPair)
        val signatureStart = LENGTH_PREFIX_BYTES + externalHeaderSize(bootstrapHeader())
        val tampered = frame.copyOf()
        tampered[signatureStart] = flipLowestBit(tampered[signatureStart])

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decodeBootstrap(tampered, MAX_FRAME_BYTES)
        }
    }

    @Test
    fun decodeBootstrapRejectsPayloadThatIsNotTwoKeyArray() {
        val ed25519KeyPair = Ed25519.generate()
        val oneKeyPayload =
            CborEncoder.encode(
                CborValue.Array(listOf(CborValue.Bytes(ByteArray(KEY_BYTES)))),
            )
        val frame =
            FrameBodyCodec.encode(
                header = bootstrapHeader(),
                payload = oneKeyPayload,
                sign = { signedBytes -> Ed25519.sign(ed25519KeyPair.privateKey, signedBytes) },
                maxFrameBytes = MAX_FRAME_BYTES,
            )

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decodeBootstrap(frame, MAX_FRAME_BYTES)
        }
    }

    @Test
    fun decodeBootstrapRejectsFrameAboveLimit() {
        val frame = bootstrapFrame(X25519.generate().publicKey, Ed25519.generate())
        val limitBelowDeclaredLength = frame.size - LENGTH_PREFIX_BYTES - 1

        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.decodeBootstrap(frame, limitBelowDeclaredLength)
        }
    }

    private fun bootstrapHeader(): ExternalFrameHeader =
        ExternalFrameHeader(
            senderId = SENDER_ID,
            requestId = REQUEST_ID,
            frameNumber = FRAME_NUMBER,
            uploadId = null,
            responderLabel = null,
            bootstrapSigningPublicKey = null,
        )

    private fun bootstrapFrame(
        x25519PublicKey: ByteArray,
        ed25519KeyPair: Ed25519KeyPair,
    ): ByteArray =
        FrameBodyCodec.encode(
            header = bootstrapHeader(),
            payload = FrameBodyCodec.bootstrapPayload(x25519PublicKey, ed25519KeyPair.publicKey),
            sign = { signedBytes -> Ed25519.sign(ed25519KeyPair.privateKey, signedBytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun ordinaryHeader(): ExternalFrameHeader =
        ExternalFrameHeader(
            senderId = SENDER_ID,
            requestId = REQUEST_ID,
            frameNumber = FRAME_NUMBER,
            uploadId = UPLOAD_ID,
            responderLabel = RESPONDER_LABEL,
            bootstrapSigningPublicKey = null,
        )

    private fun encodeFrame(
        header: ExternalFrameHeader,
        payload: ByteArray,
        keyPair: Ed25519KeyPair,
    ): ByteArray =
        FrameBodyCodec.encode(
            header = header,
            payload = payload,
            sign = { signedBytes -> Ed25519.sign(keyPair.privateKey, signedBytes) },
            maxFrameBytes = MAX_FRAME_BYTES,
        )

    private fun decodeFrame(
        frame: ByteArray,
        keyPair: Ed25519KeyPair,
    ): DecodedFrameBody =
        FrameBodyCodec.decode(frame, MAX_FRAME_BYTES) { signedBytes, signature ->
            Ed25519.verify(keyPair.publicKey, signedBytes, signature)
        }

    private fun externalHeaderSize(header: ExternalFrameHeader): Int = ExternalFrameHeaderCodec.encode(header).size

    private fun assertBytes(
        expected: ByteArray,
        actual: ByteArray?,
    ) {
        assertArrayEquals(expected, actual ?: EMPTY)
    }

    private fun flipLowestBit(value: Byte): Byte = (value.toInt() xor LOWEST_BIT_MASK).toByte()

    private fun lengthPrefix(length: Int): ByteArray =
        byteArrayOf(
            (length ushr (BITS_PER_BYTE * THREE_BYTES)).toByte(),
            (length ushr (BITS_PER_BYTE * TWO_BYTES)).toByte(),
            (length ushr BITS_PER_BYTE).toByte(),
            length.toByte(),
        )

    private companion object {
        const val LENGTH_PREFIX_BYTES = 4
        const val SIGNATURE_BYTES = 64
        const val KEY_BYTES = 32
        const val GCM_TAG_BYTES = 16
        const val MAX_NESTING_LEVELS = 64
        const val MAX_FRAME_BYTES = 4_096
        const val ZERO_LIMIT = 0
        const val NEGATIVE_LIMIT = -1
        const val FRAME_NUMBER = 7L
        const val SENDER_ID_CONTENT_OFFSET = 3
        const val LOWEST_BIT_MASK = 0x01
        const val BITS_PER_BYTE = 8
        const val TWO_BYTES = 2
        const val THREE_BYTES = 3
        const val X25519_KEY_FILL: Byte = 0x11
        const val ED25519_KEY_FILL: Byte = 0x22
        const val RESPONDER_LABEL = "node-alpha"

        val EMPTY = ByteArray(0)
        val SENDER_ID = ByteArray(16) { index -> (index + 1).toByte() }
        val REQUEST_ID = ByteArray(16) { index -> (index + 17).toByte() }
        val UPLOAD_ID = ByteArray(16) { index -> (index + 33).toByte() }
        val PAYLOAD = byteArrayOf(0x10, 0x20, 0x30, 0x40)
        val PLAINTEXT = byteArrayOf(0x50, 0x60, 0x70, 0x80.toByte(), 0x01, 0x02)
    }

    @Test
    fun `encode and decode accept a body exactly at the frame limit`() {
        val keyPair = Ed25519.generate()
        val header = ordinaryHeader()
        val frame = encodeFrame(header, PAYLOAD, keyPair)
        val exactLimit = frame.size - LENGTH_PREFIX_BYTES

        val encoded =
            FrameBodyCodec.encode(
                header = header,
                payload = PAYLOAD,
                sign = { signedBytes -> Ed25519.sign(keyPair.privateKey, signedBytes) },
                maxFrameBytes = exactLimit,
            )
        val decoded =
            FrameBodyCodec.decode(encoded, exactLimit) { signedBytes, signature ->
                Ed25519.verify(keyPair.publicKey, signedBytes, signature)
            }

        assertArrayEquals(PAYLOAD, decoded.payload)
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.encode(
                header = header,
                payload = PAYLOAD,
                sign = { signedBytes -> Ed25519.sign(keyPair.privateKey, signedBytes) },
                maxFrameBytes = exactLimit - 1,
            )
        }
    }

    @Test
    fun `encode rejects a signer that returns a non 64 byte signature`() {
        assertThrows(ProtocolEncodingException::class.java) {
            FrameBodyCodec.encode(
                header = ordinaryHeader(),
                payload = PAYLOAD,
                sign = { ByteArray(SIGNATURE_BYTES - 1) },
                maxFrameBytes = MAX_FRAME_BYTES,
            )
        }
    }

    @Test
    fun `encrypted payload uses a fresh encapsulation for every call`() {
        val recipient = X25519.generate()

        val first = FrameBodyCodec.encryptedPayload(recipient.publicKey, PAYLOAD)
        val second = FrameBodyCodec.encryptedPayload(recipient.publicKey, PAYLOAD)

        assertFalse(first.contentEquals(second))
        assertArrayEquals(PAYLOAD, FrameBodyCodec.decryptedPayload(recipient.privateKey, first))
        assertArrayEquals(PAYLOAD, FrameBodyCodec.decryptedPayload(recipient.privateKey, second))
    }

    @Test
    fun `decodePrefix rejects an out of range offset`() {
        val bytes = byteArrayOf(0x01)

        assertThrows(ProtocolEncodingException::class.java) { CborDecoder.decodePrefix(bytes, -1) }
        assertThrows(ProtocolEncodingException::class.java) { CborDecoder.decodePrefix(bytes, 2) }
    }

    @Test
    fun `decodePrefix still bounds the array nesting depth`() {
        var nested = ByteArray(0)
        repeat(MAX_NESTING_LEVELS + 1) { nested = byteArrayOf(0x81.toByte()) + nested }

        assertThrows(ProtocolEncodingException::class.java) { CborDecoder.decodePrefix(nested) }
    }
}
