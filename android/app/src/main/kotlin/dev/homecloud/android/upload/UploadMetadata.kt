package dev.homecloud.android.upload

import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ProtocolEncodingException

/**
 * The five upload metadata arguments of one version, in wire order (`INT-NET-017 / 2`,
 * `INT-NET-084 / 1`, `FR-UP-002 / 1`, `FR-UP-019 / 2`, `FR-CAT-012 / 3`).
 *
 * Equality and hashing compare the SHA-256 digest by content, and the textual form never contains
 * the digest bytes.
 */
internal data class UploadMetadata(
    val originalName: String,
    val timestampMillis: Long,
    val originalSizeBytes: Long,
    val compressedSizeBytes: Long,
    val sha256: ByteArray,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is UploadMetadata) {
            return false
        }
        return originalName == other.originalName &&
            timestampMillis == other.timestampMillis &&
            originalSizeBytes == other.originalSizeBytes &&
            compressedSizeBytes == other.compressedSizeBytes &&
            sha256.contentEquals(other.sha256)
    }

    override fun hashCode(): Int {
        var result = originalName.hashCode()
        result = HASH_MULTIPLIER * result + timestampMillis.hashCode()
        result = HASH_MULTIPLIER * result + originalSizeBytes.hashCode()
        result = HASH_MULTIPLIER * result + compressedSizeBytes.hashCode()
        result = HASH_MULTIPLIER * result + sha256.contentHashCode()
        return result
    }

    override fun toString(): String =
        "UploadMetadata(originalName=$originalName, timestampMillis=$timestampMillis, " +
            "originalSizeBytes=$originalSizeBytes, compressedSizeBytes=$compressedSizeBytes)"
}

/**
 * Encodes and decodes the positional upload metadata arguments (`INT-NET-017 / 2`,
 * `INT-NET-084 / 1`, `FR-UP-002 / 1`, `FR-UP-019 / 2`, `FR-CAT-012 / 3`).
 *
 * The encoded order is `source_name`, `timestamp_ms`, `original_size`, `compressed_size` and
 * `sha256`. The SRS frame-size bound B(n) is a wire-envelope check; it does not allow the client's
 * compressed size to grow beyond the original source size, so the codec requires
 * `compressed_size <= original_size` in both directions.
 */
internal object UploadMetadataCodec {
    /**
     * Encodes the five arguments in the order fixed by `INT-NET-017 / 2` and `INT-NET-084 / 1`.
     *
     * A blank name, a digest whose length is not 32 bytes, a negative timestamp or size, and a
     * compressed size above the original size are rejected.
     */
    fun encode(metadata: UploadMetadata): List<CborValue> {
        require(metadata.originalName.isNotEmpty()) { "$SOURCE_NAME_FIELD must not be empty" }
        require(metadata.sha256.size == SHA256_BYTES) { "$SHA256_FIELD must contain $SHA256_BYTES bytes" }
        require(metadata.timestampMillis >= 0) { "$TIMESTAMP_FIELD must not be negative" }
        require(metadata.originalSizeBytes in VALID_SIZE_RANGE) { "$ORIGINAL_SIZE_FIELD is out of range" }
        require(metadata.compressedSizeBytes in VALID_SIZE_RANGE) { "$COMPRESSED_SIZE_FIELD is out of range" }
        require(metadata.compressedSizeBytes <= metadata.originalSizeBytes) {
            "$COMPRESSED_SIZE_FIELD must not exceed $ORIGINAL_SIZE_FIELD"
        }
        return listOf(
            CborValue.Text(metadata.originalName),
            CborValue.UInt(metadata.timestampMillis),
            CborValue.UInt(metadata.originalSizeBytes),
            CborValue.UInt(metadata.compressedSizeBytes),
            CborValue.Bytes(metadata.sha256),
        )
    }

    /**
     * Decodes and validates a five-element argument list (`INT-NET-017 / 2`, `INT-NET-084 / 1`).
     *
     * A wrong element count, a mismatched element type, a negative timestamp or size, a digest
     * whose length is not 32 bytes, an empty name, and a compressed size above the original size
     * are rejected.
     */
    fun decode(arguments: List<CborValue>): UploadMetadata {
        if (arguments.size != ARGUMENT_COUNT) {
            throw ProtocolEncodingException(
                "$UPLOAD_METADATA_FIELD must contain exactly $ARGUMENT_COUNT elements",
            )
        }
        val metadata =
            UploadMetadata(
                originalName = arguments[0].nonEmptyText(SOURCE_NAME_FIELD),
                timestampMillis = arguments[1].timestamp(TIMESTAMP_FIELD),
                originalSizeBytes = arguments[2].unsignedSize(ORIGINAL_SIZE_FIELD),
                compressedSizeBytes = arguments[3].unsignedSize(COMPRESSED_SIZE_FIELD),
                sha256 = arguments[4].sha256Digest(SHA256_FIELD),
            )
        if (metadata.compressedSizeBytes > metadata.originalSizeBytes) {
            throw ProtocolEncodingException("$COMPRESSED_SIZE_FIELD must not exceed $ORIGINAL_SIZE_FIELD")
        }
        return metadata
    }
}

private fun CborValue.nonEmptyText(field: String): String {
    val text =
        (this as? CborValue.Text)?.value
            ?: throw ProtocolEncodingException("$field must be a text string")
    if (text.isEmpty()) {
        throw ProtocolEncodingException("$field must not be empty")
    }
    return text
}

private fun CborValue.timestamp(field: String): Long {
    val value =
        when (this) {
            is CborValue.UInt -> value
            is CborValue.NInt -> value
            else -> throw ProtocolEncodingException("$field must be an integer")
        }
    if (value < 0) {
        throw ProtocolEncodingException("$field must not be negative")
    }
    return value
}

private fun CborValue.unsignedSize(field: String): Long {
    val value =
        (this as? CborValue.UInt)?.value
            ?: throw ProtocolEncodingException("$field must be an unsigned integer")
    if (value < 0) {
        throw ProtocolEncodingException("$field must not be negative")
    }
    return value
}

private fun CborValue.sha256Digest(field: String): ByteArray {
    val bytes =
        (this as? CborValue.Bytes)?.value
            ?: throw ProtocolEncodingException("$field must be a byte string")
    if (bytes.size != SHA256_BYTES) {
        throw ProtocolEncodingException("$field must contain $SHA256_BYTES bytes")
    }
    return bytes.copyOf()
}

private const val ARGUMENT_COUNT = 5
private const val SHA256_BYTES = 32
private const val HASH_MULTIPLIER = 31

private val VALID_SIZE_RANGE = 0L..Long.MAX_VALUE

private const val UPLOAD_METADATA_FIELD = "upload metadata"
private const val SOURCE_NAME_FIELD = "source_name"
private const val TIMESTAMP_FIELD = "timestamp_ms"
private const val ORIGINAL_SIZE_FIELD = "original_size"
private const val COMPRESSED_SIZE_FIELD = "compressed_size"
private const val SHA256_FIELD = "sha256"
