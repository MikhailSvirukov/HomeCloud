package dev.homecloud.android.upload

import dev.homecloud.android.protocol.CborDecoder
import dev.homecloud.android.protocol.CborEncoder
import dev.homecloud.android.protocol.CborValue
import dev.homecloud.android.protocol.ProtocolEncodingException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class UploadMetadataTest {
    @Test
    fun encode_produces_the_five_wire_arguments_in_order() {
        val metadata = metadata()

        val arguments = UploadMetadataCodec.encode(metadata)

        assertEquals(ARGUMENT_COUNT, arguments.size)
        assertEquals(CborValue.Text(ORIGINAL_NAME), arguments[0])
        assertEquals(CborValue.UInt(TIMESTAMP_MILLIS), arguments[1])
        assertEquals(CborValue.UInt(ORIGINAL_SIZE_BYTES), arguments[2])
        assertEquals(CborValue.UInt(COMPRESSED_SIZE_BYTES), arguments[3])
        assertArrayEquals(metadata.sha256, (arguments[4] as CborValue.Bytes).value)
    }

    @Test
    fun round_trip_preserves_every_field() {
        val metadata = metadata(originalName = "clip.mp4", sha256 = digest(0x7F))

        val decoded = UploadMetadataCodec.decode(UploadMetadataCodec.encode(metadata))

        assertEquals(metadata.originalName, decoded.originalName)
        assertEquals(metadata.timestampMillis, decoded.timestampMillis)
        assertEquals(metadata.originalSizeBytes, decoded.originalSizeBytes)
        assertEquals(metadata.compressedSizeBytes, decoded.compressedSizeBytes)
        assertArrayEquals(metadata.sha256, decoded.sha256)
    }

    @Test
    fun equality_and_hash_code_compare_the_digest_by_content() {
        val first = metadata()
        val second = metadata(sha256 = first.sha256.copyOf())

        assertEquals(first, second)
        assertEquals(first.hashCode(), second.hashCode())
    }

    @Test
    fun encode_rejects_an_empty_name() {
        assertEncodeRejected(metadata(originalName = ""), SOURCE_NAME_FIELD)
    }

    @Test
    fun encode_rejects_a_digest_shorter_than_32_bytes() {
        assertEncodeRejected(metadata(sha256 = ByteArray(SHA256_BYTES - 1)), SHA256_FIELD)
    }

    @Test
    fun encode_rejects_a_digest_longer_than_32_bytes() {
        assertEncodeRejected(metadata(sha256 = ByteArray(SHA256_BYTES + 1)), SHA256_FIELD)
    }

    @Test
    fun encode_rejects_a_negative_original_size() {
        assertEncodeRejected(
            metadata(originalSizeBytes = -1L, compressedSizeBytes = 0L),
            ORIGINAL_SIZE_FIELD,
        )
    }

    @Test
    fun encode_rejects_a_negative_compressed_size() {
        assertEncodeRejected(metadata(compressedSizeBytes = -1L), COMPRESSED_SIZE_FIELD)
    }

    @Test
    fun decode_rejects_four_arguments() {
        assertDecodeRejected(validArguments().take(ARGUMENT_COUNT - 1), UPLOAD_METADATA_FIELD)
    }

    @Test
    fun decode_rejects_six_arguments() {
        assertDecodeRejected(validArguments() + CborValue.UInt(0L), UPLOAD_METADATA_FIELD)
    }

    @Test
    fun decode_rejects_text_where_a_size_is_expected() {
        val arguments =
            validArguments().toMutableList().also { elements ->
                elements[ORIGINAL_SIZE_INDEX] = CborValue.Text(ORIGINAL_SIZE_BYTES.toString())
            }

        assertDecodeRejected(arguments, ORIGINAL_SIZE_FIELD)
    }

    @Test
    fun decode_rejects_a_negative_size() {
        val arguments =
            validArguments().toMutableList().also { elements ->
                elements[ORIGINAL_SIZE_INDEX] = CborValue.NInt(-1L)
            }

        assertDecodeRejected(arguments, ORIGINAL_SIZE_FIELD)
    }

    @Test
    fun decode_rejects_a_digest_with_the_wrong_length() {
        val arguments =
            validArguments().toMutableList().also { elements ->
                elements[SHA256_INDEX] = CborValue.Bytes(ByteArray(SHA256_BYTES - 1))
            }

        assertDecodeRejected(arguments, SHA256_FIELD)
    }

    @Test
    fun decode_rejects_an_empty_name() {
        val arguments =
            validArguments().toMutableList().also { elements ->
                elements[SOURCE_NAME_INDEX] = CborValue.Text("")
            }

        assertDecodeRejected(arguments, SOURCE_NAME_FIELD)
    }

    @Test
    fun decode_rejects_a_non_text_name() {
        val arguments = validArguments().toMutableList()
        arguments[SOURCE_NAME_INDEX] = CborValue.UInt(1)

        assertDecodeRejected(arguments, SOURCE_NAME_FIELD)
    }

    @Test
    fun decode_rejects_a_non_integer_timestamp() {
        val arguments = validArguments().toMutableList()
        arguments[TIMESTAMP_INDEX] = CborValue.Text("not-a-number")

        assertDecodeRejected(arguments, TIMESTAMP_FIELD)
    }

    @Test
    fun decode_rejects_a_non_bytes_digest() {
        val arguments = validArguments().toMutableList()
        arguments[SHA256_INDEX] = CborValue.Text("not-bytes")

        assertDecodeRejected(arguments, SHA256_FIELD)
    }

    @Test
    fun `a compressed size above the original size is accepted`() {
        val metadata =
            metadata(
                originalSizeBytes = COMPRESSED_SIZE_BYTES,
                compressedSizeBytes = COMPRESSED_SIZE_BYTES + 1,
            )

        val decoded = UploadMetadataCodec.decode(UploadMetadataCodec.encode(metadata))

        assertEquals(metadata.compressedSizeBytes, decoded.compressedSizeBytes)
    }

    @Test
    fun `a name at the configured byte limit is accepted`() {
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = ORIGINAL_SIZE_BYTES, maxNameBytes = 8)
        val metadata = metadata(originalName = "12345678")

        val decoded = UploadMetadataCodec.decode(UploadMetadataCodec.encode(metadata, limits), limits)

        assertEquals(metadata.originalName, decoded.originalName)
    }

    @Test
    fun `a name above the configured byte limit is rejected`() {
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = ORIGINAL_SIZE_BYTES, maxNameBytes = 8)

        assertThrows(IllegalArgumentException::class.java) {
            UploadMetadataCodec.encode(metadata(originalName = "123456789"), limits)
        }
    }

    @Test
    fun `a size above the configured file limit is rejected on encode and decode`() {
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = COMPRESSED_SIZE_BYTES)

        assertThrows(IllegalArgumentException::class.java) {
            UploadMetadataCodec.encode(metadata(), limits)
        }
        assertThrows(ProtocolEncodingException::class.java) {
            UploadMetadataCodec.decode(validArguments(), limits)
        }
    }

    @Test
    fun `a negative timestamp round-trips`() {
        val metadata = metadata(timestampMillis = -1_000L)

        val decoded = UploadMetadataCodec.decode(UploadMetadataCodec.encode(metadata))

        assertEquals(metadata.timestampMillis, decoded.timestampMillis)
    }

    @Test
    fun `decode rejects a name above the configured byte limit`() {
        val encoded = UploadMetadataCodec.encode(metadata(originalName = "12345678"))
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = ORIGINAL_SIZE_BYTES, maxNameBytes = 4)

        assertThrows(ProtocolEncodingException::class.java) {
            UploadMetadataCodec.decode(encoded, limits)
        }
    }

    @Test
    fun `decode rejects a compressed size above the configured file limit`() {
        val encoded =
            UploadMetadataCodec.encode(
                metadata(originalSizeBytes = 100L, compressedSizeBytes = 200L),
            )
        val limits = UploadMetadataLimits(maxOriginalSizeBytes = 150L)

        assertThrows(ProtocolEncodingException::class.java) {
            UploadMetadataCodec.decode(encoded, limits)
        }
    }

    @Test
    fun matching_digest_round_trips_through_the_cbor_codec() {
        val metadata = metadata(sha256 = digest(0x2A))
        val bytes = CborEncoder.encode(CborValue.Array(UploadMetadataCodec.encode(metadata)))

        val arguments = (CborDecoder.decode(bytes) as CborValue.Array).items
        val decoded = UploadMetadataCodec.decode(arguments)

        assertArrayEquals(metadata.sha256, decoded.sha256)
        assertEquals(metadata, decoded)
    }

    private fun metadata(
        originalName: String = ORIGINAL_NAME,
        timestampMillis: Long = TIMESTAMP_MILLIS,
        originalSizeBytes: Long = ORIGINAL_SIZE_BYTES,
        compressedSizeBytes: Long = COMPRESSED_SIZE_BYTES,
        sha256: ByteArray = digest(),
    ): UploadMetadata =
        UploadMetadata(
            originalName = originalName,
            timestampMillis = timestampMillis,
            originalSizeBytes = originalSizeBytes,
            compressedSizeBytes = compressedSizeBytes,
            sha256 = sha256,
        )

    private fun validArguments(): List<CborValue> = UploadMetadataCodec.encode(metadata())

    private fun assertEncodeRejected(
        metadata: UploadMetadata,
        field: String,
    ) {
        val error =
            assertThrows(IllegalArgumentException::class.java) {
                UploadMetadataCodec.encode(metadata)
            }

        assertTrue(error.message.orEmpty().contains(field))
    }

    private fun assertDecodeRejected(
        arguments: List<CborValue>,
        field: String,
    ) {
        val error =
            assertThrows(ProtocolEncodingException::class.java) {
                UploadMetadataCodec.decode(arguments)
            }

        assertTrue(error.message.orEmpty().contains(field))
    }

    private fun digest(fill: Int = 0x5A): ByteArray = ByteArray(SHA256_BYTES) { fill.toByte() }
}

private const val ARGUMENT_COUNT = 5
private const val SHA256_BYTES = 32
private const val SOURCE_NAME_INDEX = 0
private const val TIMESTAMP_INDEX = 1
private const val ORIGINAL_SIZE_INDEX = 2
private const val SHA256_INDEX = 4
private const val ORIGINAL_NAME = "IMG_0001.jpg"
private const val TIMESTAMP_MILLIS = 1_700_000_000_000L
private const val ORIGINAL_SIZE_BYTES = 4_096L
private const val COMPRESSED_SIZE_BYTES = 2_048L

private const val UPLOAD_METADATA_FIELD = "upload metadata"
private const val SOURCE_NAME_FIELD = "source_name"
private const val TIMESTAMP_FIELD = "timestamp_ms"
private const val ORIGINAL_SIZE_FIELD = "original_size"
private const val COMPRESSED_SIZE_FIELD = "compressed_size"
private const val SHA256_FIELD = "sha256"
