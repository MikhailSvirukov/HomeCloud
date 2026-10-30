package dev.homecloud.android.upload

import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ProtocolEncodingException

/**
 * The five upload metadata arguments of one version, in wire order (`INT-NET-052 / 4`, `INT-NET-053 / 2`, `INT-NET-084 / 1`, `INT-NET-038 / 2`).
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
        "UploadMetadata(timestampMillis=$timestampMillis, " +
            "originalSizeBytes=$originalSizeBytes, compressedSizeBytes=$compressedSizeBytes)"
}

/** Configured bounds for the upload metadata (`CFG-CL-020`, `CFG-CL-032`). */
internal data class UploadMetadataLimits(
    val maxOriginalSizeBytes: Long,
    val maxNameBytes: Int = DEFAULT_MAX_NAME_BYTES,
) {
    init {
        require(maxOriginalSizeBytes >= 1L) { "maxOriginalSizeBytes must be positive" }
        require(maxNameBytes in 1..MAX_NAME_BYTES_RANGE) { "maxNameBytes is out of range" }
    }
}

/**
 * Encodes and decodes the positional upload metadata arguments (`INT-NET-052 / 4`, `INT-NET-053 / 2`, `INT-NET-084 / 1`, `INT-NET-038 / 2`).
 *
 * The encoded order is `source_name`, `timestamp_ms`, `original_size`, `compressed_size` and
 * `sha256`. The source name is bounded by the configured source-name limit (`CFG-CL-032`) and both
 * sizes by the configured file-size limit (`CFG-CL-020`); the compressed size may exceed the original
 * size, because Zstandard can expand incompressible input.
 */
internal object UploadMetadataCodec {
    /** Default name limit (`CFG-CL-032`) and no original-size bound; callers pass the configured `CFG-CL-020` value. */
    val DEFAULT_LIMITS = UploadMetadataLimits(maxOriginalSizeBytes = Long.MAX_VALUE)

    /**
     * Encodes the five arguments in the order fixed by `INT-NET-052 / 4` and `INT-NET-084 / 1`.
     *
     * An empty name, a name above the configured byte limit, a digest whose length is not 32 bytes,
     * a negative timestamp or size, and a size above the configured limit are rejected.
     */
    fun encode(
        metadata: UploadMetadata,
        limits: UploadMetadataLimits = DEFAULT_LIMITS,
    ): List<CborValue> {
        require(metadata.originalName.isNotEmpty()) { "$SOURCE_NAME_FIELD must not be empty" }
        require(metadata.originalName.toByteArray(Charsets.UTF_8).size <= limits.maxNameBytes) {
            "$SOURCE_NAME_FIELD exceeds the configured limit"
        }
        require(metadata.sha256.size == SHA256_BYTES) { "$SHA256_FIELD must contain $SHA256_BYTES bytes" }
        require(metadata.originalSizeBytes >= 0L) { "$ORIGINAL_SIZE_FIELD must not be negative" }
        require(metadata.originalSizeBytes <= limits.maxOriginalSizeBytes) { "$ORIGINAL_SIZE_FIELD exceeds the configured limit" }
        require(metadata.compressedSizeBytes >= 0L) { "$COMPRESSED_SIZE_FIELD must not be negative" }
        require(metadata.compressedSizeBytes <= limits.maxOriginalSizeBytes) { "$COMPRESSED_SIZE_FIELD exceeds the configured limit" }
        return listOf(
            CborValue.Text(metadata.originalName),
            metadata.timestampMillis.toCborInteger(),
            CborValue.UInt(metadata.originalSizeBytes),
            CborValue.UInt(metadata.compressedSizeBytes),
            CborValue.Bytes(metadata.sha256),
        )
    }

    /**
     * Decodes and validates a five-element argument list (`INT-NET-053 / 2`, `INT-NET-084 / 1`).
     *
     * A wrong element count, a mismatched element type, a negative timestamp or size, a digest
     * whose length is not 32 bytes, an empty name, a name above the configured byte limit, and a
     * size above the configured limit are rejected.
     */
    fun decode(
        arguments: List<CborValue>,
        limits: UploadMetadataLimits = DEFAULT_LIMITS,
    ): UploadMetadata {
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
        if (metadata.originalSizeBytes > limits.maxOriginalSizeBytes) {
            throw ProtocolEncodingException("$ORIGINAL_SIZE_FIELD exceeds the configured limit")
        }
        if (metadata.compressedSizeBytes > limits.maxOriginalSizeBytes) {
            throw ProtocolEncodingException("$COMPRESSED_SIZE_FIELD exceeds the configured limit")
        }
        if (metadata.originalName.toByteArray(Charsets.UTF_8).size > limits.maxNameBytes) {
            throw ProtocolEncodingException("$SOURCE_NAME_FIELD exceeds the configured limit")
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

private const val UPLOAD_METADATA_FIELD = "upload metadata"
private const val DEFAULT_MAX_NAME_BYTES = 4_096
private const val MAX_NAME_BYTES_RANGE = 65_535
private const val SOURCE_NAME_FIELD = "source_name"
private const val TIMESTAMP_FIELD = "timestamp_ms"
private const val ORIGINAL_SIZE_FIELD = "original_size"
private const val COMPRESSED_SIZE_FIELD = "compressed_size"
private const val SHA256_FIELD = "sha256"

private fun Long.toCborInteger(): CborValue = if (this >= 0L) CborValue.UInt(this) else CborValue.NInt(this)
