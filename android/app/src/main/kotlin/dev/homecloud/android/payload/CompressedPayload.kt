package dev.homecloud.android.payload

/**
 * The complete compressed representation of one media version, held as a bounded chain of payload
 * buffers for the length of the upload attempt.
 *
 * The payload owns the buffers in [blocks]; callers read them but never modify them. The full
 * compressed size is fixed when the payload is created, so the first upload frame can be built
 * only after the entire source has been compressed (FR-AND-019 / 1, FR-AND-020 / 1, FR-UP-002 / 1).
 */
internal class CompressedPayload internal constructor(
    blocks: List<ByteArray>,
) {
    /** The buffers in order; every buffer is non-empty and at most the configured size. */
    val blocks: List<ByteArray> = blocks.toList()

    /** The full compressed size, known before the first frame is built. */
    val totalBytes: Long = this.blocks.sumOf { it.size.toLong() }

    /** The number of buffers in the chain. */
    val blockCount: Int
        get() = blocks.size
}
