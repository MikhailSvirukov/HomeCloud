package dev.homecloud.android.protocol

import dev.homecloud.android.crypto.Ed25519
import dev.homecloud.android.crypto.Hpke

/**
 * An application frame body split into its external header, its 64-byte signature and its payload
 * (`INT-NET-081`).
 *
 * The payload is either the encrypted application payload or the plaintext bootstrap payload.
 */
internal data class DecodedFrameBody(
    val header: ExternalFrameHeader,
    val signature: ByteArray,
    val payload: ByteArray,
)

/**
 * The verified plaintext bootstrap frame: header plus the dedicated server's two public keys.
 *
 * The signature proves only possession of the Ed25519 key carried in the payload, so the client
 * pins both keys without independent authentication (`INT-NET-021`, `INT-NET-025`, `INT-NET-081`).
 */
internal data class BootstrapFrame(
    val header: ExternalFrameHeader,
    val x25519PublicKey: ByteArray,
    val ed25519PublicKey: ByteArray,
)

/**
 * Composes and verifies the signed application frame body of the v1 protocol (`INT-NET-018`,
 * `INT-NET-019`, `INT-NET-020`, `INT-NET-028`, `INT-NET-081`).
 *
 * A body is the four-byte big-endian length prefix, the deterministic external header, the 64-byte
 * Ed25519 signature and the payload. The prefix counts the header, the signature and the payload
 * and excludes itself. The signature covers the prefix, the header and the payload in that order,
 * and verification always completes before a decoded payload is released.
 */
internal object FrameBodyCodec {
    /**
     * Builds a frame body from its header and payload, signing the length prefix, the header and
     * the payload with the caller's Ed25519 signer (`INT-NET-002`, `INT-NET-003`, `INT-NET-018`,
     * `INT-NET-019`, `INT-NET-081`).
     *
     * The declared body length is checked against the configured maximum before the frame is
     * allocated. The returned bytes already include the length prefix, so they are written to the
     * transport unchanged and must never be framed a second time.
     */
    fun encode(
        header: ExternalFrameHeader,
        payload: ByteArray,
        sign: (ByteArray) -> ByteArray,
        maxFrameBytes: Int,
    ): ByteArray {
        requirePositiveLimit(maxFrameBytes)
        val headerBytes = ExternalFrameHeaderCodec.encode(header)
        val bodyLength = bodyLength(headerBytes, payload)
        if (bodyLength > maxFrameBytes.toLong()) {
            throw ProtocolEncodingException("frame exceeds the maximum frame size")
        }
        val signature = sign(signedBytes(headerBytes, payload))
        if (signature.size != SIGNATURE_BYTES) {
            throw ProtocolEncodingException("signature must contain $SIGNATURE_BYTES bytes")
        }
        val frame = ByteArray(LENGTH_PREFIX_BYTES + bodyLength.toInt())
        writeLength(bodyLength.toInt(), frame)
        headerBytes.copyInto(frame, LENGTH_PREFIX_BYTES)
        signature.copyInto(frame, LENGTH_PREFIX_BYTES + headerBytes.size)
        payload.copyInto(frame, LENGTH_PREFIX_BYTES + headerBytes.size + SIGNATURE_BYTES)
        return frame
    }

    /**
     * Validates the length prefix and the configured maximum, parses the external header, splits
     * the signature and the payload and verifies the signature over the prefix, the header and the
     * payload (`INT-NET-004`, `INT-NET-020`, `INT-NET-081`).
     *
     * The payload is returned only after verification succeeds.
     */
    fun decode(
        frame: ByteArray,
        maxFrameBytes: Int,
        verify: (signedBytes: ByteArray, signature: ByteArray) -> Boolean,
    ): DecodedFrameBody {
        val body = split(frame, maxFrameBytes)
        if (!verify(signedBytes(body.headerBytes, body.payload), body.signature)) {
            throw ProtocolEncodingException("frame signature verification failed")
        }
        return DecodedFrameBody(header = body.header, signature = body.signature, payload = body.payload)
    }

    /**
     * Decodes the dedicated server's first-contact bootstrap frame, reads the two public keys from
     * its plaintext payload and verifies the signature with the Ed25519 key carried in that payload
     * (`INT-NET-021`, `INT-NET-024`, `INT-NET-025`, `INT-NET-081`).
     *
     * The size limit and split follow [decode], and the payload remains the only plaintext
     * application payload of the protocol.
     */
    fun decodeBootstrap(
        frame: ByteArray,
        maxFrameBytes: Int,
    ): BootstrapFrame {
        val body = split(frame, maxFrameBytes)
        if (body.header.senderId == null || body.header.bootstrapSigningPublicKey != null) {
            throw ProtocolEncodingException("bootstrap frame carries the wrong header shape")
        }
        val keys = bootstrapKeys(body.payload)
        if (!Ed25519.verify(keys.second, signedBytes(body.headerBytes, body.payload), body.signature)) {
            throw ProtocolEncodingException("frame signature verification failed")
        }
        return BootstrapFrame(
            header = body.header,
            x25519PublicKey = keys.first,
            ed25519PublicKey = keys.second,
        )
    }

    /**
     * The exact bytes covered by the Ed25519 signature: the four-byte length prefix followed by the
     * external header and the payload (`INT-NET-019`, `INT-NET-081`).
     */
    fun signedBytes(
        headerBytes: ByteArray,
        payload: ByteArray,
    ): ByteArray {
        val bodyLength = bodyLength(headerBytes, payload)
        if (bodyLength > Int.MAX_VALUE) {
            throw ProtocolEncodingException("frame exceeds the supported size")
        }
        val signed = ByteArray(LENGTH_PREFIX_BYTES + headerBytes.size + payload.size)
        writeLength(bodyLength.toInt(), signed)
        headerBytes.copyInto(signed, LENGTH_PREFIX_BYTES)
        payload.copyInto(signed, LENGTH_PREFIX_BYTES + headerBytes.size)
        return signed
    }

    /**
     * Seals a plaintext with HPKE Base mode and the empty `aad` and `info` required by v1, returning
     * the 32-byte encapsulated key followed by the AES-256-GCM ciphertext and tag (`INT-NET-024`,
     * `INT-NET-027`, `INT-NET-081`).
     */
    fun encryptedPayload(
        recipientPublicKey: ByteArray,
        plaintext: ByteArray,
    ): ByteArray {
        val sealed = Hpke.seal(recipientPublicKey, EMPTY, plaintext, EMPTY)
        return concat(sealed.encapsulatedKey, sealed.ciphertext)
    }

    /**
     * Opens a payload produced by [encryptedPayload] with the recipient's X25519 private key and the
     * empty `aad` and `info` required by v1 (`INT-NET-033`).
     *
     * A payload shorter than the encapsulated key and the authentication tag is rejected as
     * malformed, while cryptographic failures propagate unchanged.
     */
    fun decryptedPayload(
        recipientPrivateKey: ByteArray,
        payload: ByteArray,
    ): ByteArray {
        if (payload.size < KEY_BYTES + GCM_TAG_BYTES) {
            throw ProtocolEncodingException("encrypted payload is too short")
        }
        val encapsulatedKey = payload.copyOfRange(0, KEY_BYTES)
        val ciphertext = payload.copyOfRange(KEY_BYTES, payload.size)
        return Hpke.open(recipientPrivateKey, encapsulatedKey, EMPTY, ciphertext, EMPTY)
    }

    /**
     * Builds the plaintext bootstrap payload, a deterministic CBOR array of the X25519 public key
     * followed by the Ed25519 public key (`INT-NET-034`, `INT-NET-081`).
     */
    fun bootstrapPayload(
        x25519PublicKey: ByteArray,
        ed25519PublicKey: ByteArray,
    ): ByteArray {
        requireKeyLength(x25519PublicKey, X25519_PUBLIC_KEY_FIELD)
        requireKeyLength(ed25519PublicKey, ED25519_PUBLIC_KEY_FIELD)
        return CborEncoder.encode(
            CborValue.Array(
                listOf(
                    CborValue.Bytes(x25519PublicKey),
                    CborValue.Bytes(ed25519PublicKey),
                ),
            ),
        )
    }

    /**
     * Reads the two 32-byte public keys from a bootstrap payload in X25519 then Ed25519 order
     * (`INT-NET-025`, `INT-NET-034`, `INT-NET-081`).
     */
    fun bootstrapKeys(payload: ByteArray): Pair<ByteArray, ByteArray> {
        val elements = CborDecoder.decode(payload).asBootstrapArray()
        if (elements.size != BOOTSTRAP_ELEMENT_COUNT) {
            throw ProtocolEncodingException(
                "bootstrap payload must contain exactly $BOOTSTRAP_ELEMENT_COUNT keys",
            )
        }
        return Pair(
            elements[0].bootstrapKey(X25519_PUBLIC_KEY_FIELD),
            elements[1].bootstrapKey(ED25519_PUBLIC_KEY_FIELD),
        )
    }

    private fun split(
        frame: ByteArray,
        maxFrameBytes: Int,
    ): SplitFrameBody {
        requirePositiveLimit(maxFrameBytes)
        if (frame.size < LENGTH_PREFIX_BYTES) {
            throw ProtocolEncodingException("frame is shorter than the length prefix")
        }
        val declaredLength = readLength(frame)
        if (declaredLength != (frame.size - LENGTH_PREFIX_BYTES).toLong()) {
            throw ProtocolEncodingException("frame length does not match the declared length")
        }
        if (declaredLength > maxFrameBytes.toLong()) {
            throw ProtocolEncodingException("frame exceeds the maximum frame size")
        }
        val decodedHeader = CborDecoder.decodePrefix(frame, LENGTH_PREFIX_BYTES)
        val headerEnd = LENGTH_PREFIX_BYTES + decodedHeader.consumedBytes
        if (frame.size - headerEnd < SIGNATURE_BYTES) {
            throw ProtocolEncodingException("frame is too short to contain the signature")
        }
        val signatureEnd = headerEnd + SIGNATURE_BYTES
        val headerBytes = frame.copyOfRange(LENGTH_PREFIX_BYTES, headerEnd)
        return SplitFrameBody(
            headerBytes = headerBytes,
            header = ExternalFrameHeaderCodec.decode(headerBytes),
            signature = frame.copyOfRange(headerEnd, signatureEnd),
            payload = frame.copyOfRange(signatureEnd, frame.size),
        )
    }
}

private class SplitFrameBody(
    val headerBytes: ByteArray,
    val header: ExternalFrameHeader,
    val signature: ByteArray,
    val payload: ByteArray,
)

private fun bodyLength(
    headerBytes: ByteArray,
    payload: ByteArray,
): Long = headerBytes.size.toLong() + SIGNATURE_BYTES + payload.size.toLong()

private fun requirePositiveLimit(maxFrameBytes: Int) {
    if (maxFrameBytes <= 0) {
        throw ProtocolEncodingException("maximum frame size must be positive")
    }
}

private fun requireKeyLength(
    key: ByteArray,
    field: String,
) {
    if (key.size != KEY_BYTES) {
        throw ProtocolEncodingException("$field must contain $KEY_BYTES bytes")
    }
}

private fun CborValue.asBootstrapArray(): List<CborValue> =
    (this as? CborValue.Array)?.items
        ?: throw ProtocolEncodingException("bootstrap payload must be an array")

private fun CborValue.bootstrapKey(field: String): ByteArray {
    val key =
        (this as? CborValue.Bytes)?.value
            ?: throw ProtocolEncodingException("$field must be a byte string")
    if (key.size != KEY_BYTES) {
        throw ProtocolEncodingException("$field must contain $KEY_BYTES bytes")
    }
    return key
}

private fun concat(vararg parts: ByteArray): ByteArray {
    val output = ByteArray(parts.sumOf { part -> part.size })
    var offset = 0
    for (part in parts) {
        part.copyInto(output, offset)
        offset += part.size
    }
    return output
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

private val EMPTY = ByteArray(0)

private const val LENGTH_PREFIX_BYTES = 4
private const val SIGNATURE_BYTES = 64
private const val KEY_BYTES = 32
private const val GCM_TAG_BYTES = 16
private const val BOOTSTRAP_ELEMENT_COUNT = 2
private const val BITS_PER_BYTE = 8
private const val BYTE_MASK = 0xFFL
private const val X25519_PUBLIC_KEY_FIELD = "bootstrap x25519 public key"
private const val ED25519_PUBLIC_KEY_FIELD = "bootstrap ed25519 public key"
