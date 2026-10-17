package dev.homecloud.android.payload

import com.github.luben.zstd.ZstdOutputStream
import java.io.InputStream
import java.io.OutputStream

/**
 * Compresses a media version with Zstandard before encryption and cuts the produced bytes into
 * payload buffers of a configured size.
 *
 * Compression reads the source in bounded chunks, so the uncompressed file is never held in
 * memory, while the complete compressed representation stays available for the upload attempt
 * (FR-AND-019 / 1, FR-AND-020 / 1, FR-UP-001 / 1). The caller's stream is never closed.
 *
 * The compression level comes from the client configuration and is validated when the compressor
 * is created; an out-of-range level is a configuration error and is not adjusted (CFG-CL-019 / 2).
 */
internal class MediaCompressor(
    private val compressionLevel: Int = DEFAULT_LEVEL,
    private val blockSizeBytes: Int = DEFAULT_BLOCK_SIZE_BYTES,
) {
    init {
        require(compressionLevel in MIN_LEVEL..MAX_LEVEL) {
            "compressionLevel must be within $MIN_LEVEL..$MAX_LEVEL but was $compressionLevel"
        }
        require(blockSizeBytes > 0) {
            "blockSizeBytes must be positive but was $blockSizeBytes"
        }
    }

    /**
     * Compresses every byte of [input] and returns the immutable chain of payload buffers.
     *
     * The full compressed size is known when this returns, so it can be sent in the first upload
     * frame before any payload byte is transferred (FR-UP-002 / 1).
     */
    fun compress(input: InputStream): CompressedPayload {
        val collector = BlockCollectingOutputStream(blockSizeBytes)
        ZstdOutputStream(collector, compressionLevel).use { zstd ->
            val readBuffer = ByteArray(READ_BUFFER_SIZE_BYTES)
            var read = input.read(readBuffer)
            while (read != END_OF_STREAM) {
                zstd.write(readBuffer, 0, read)
                read = input.read(readBuffer)
            }
        }
        return CompressedPayload(collector.collectedBlocks())
    }

    private class BlockCollectingOutputStream(
        private val blockSizeBytes: Int,
    ) : OutputStream() {
        private val sealedBlocks = mutableListOf<ByteArray>()
        private var currentBlock = ByteArray(blockSizeBytes)
        private var currentSize = 0

        override fun write(byte: Int) {
            currentBlock[currentSize] = byte.toByte()
            currentSize += 1
            sealFullBlock()
        }

        override fun write(
            bytes: ByteArray,
            offset: Int,
            length: Int,
        ) {
            var sourceOffset = offset
            var remaining = length
            while (remaining > 0) {
                val chunkSize = minOf(remaining, blockSizeBytes - currentSize)
                bytes.copyInto(
                    destination = currentBlock,
                    destinationOffset = currentSize,
                    startIndex = sourceOffset,
                    endIndex = sourceOffset + chunkSize,
                )
                currentSize += chunkSize
                sourceOffset += chunkSize
                remaining -= chunkSize
                sealFullBlock()
            }
        }

        override fun flush() = Unit

        override fun close() {
            sealPartialBlock()
        }

        fun collectedBlocks(): List<ByteArray> {
            sealPartialBlock()
            return sealedBlocks.toList()
        }

        private fun sealFullBlock() {
            if (currentSize == blockSizeBytes) {
                sealedBlocks += currentBlock
                currentBlock = ByteArray(blockSizeBytes)
                currentSize = 0
            }
        }

        private fun sealPartialBlock() {
            if (currentSize > 0) {
                sealedBlocks += currentBlock.copyOf(currentSize)
                currentBlock = ByteArray(blockSizeBytes)
                currentSize = 0
            }
        }
    }

    companion object {
        /** The Zstandard compression level used when the client configuration does not override it. */
        const val DEFAULT_LEVEL = 3

        /** The lowest compression level accepted from the client configuration. */
        const val MIN_LEVEL = 1

        /** The highest compression level accepted from the client configuration. */
        const val MAX_LEVEL = 19

        /** The payload buffer size used when the client configuration does not override it. */
        const val DEFAULT_BLOCK_SIZE_BYTES = 65536

        private const val READ_BUFFER_SIZE_BYTES = 8 * 1024
        private const val END_OF_STREAM = -1
    }
}
