package dev.homecloud.android.upload

import dev.homecloud.android.payload.CompressedPayload
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.ExternalFrameHeaderCodec

/**
 * The plaintext payloads of the upload frames of one media version, in upload order.
 *
 * Frame zero carries the logical header followed by the first compressed bytes, and every later
 * frame carries only the next compressed bytes (`FR-UP-004`, `FR-UP-005`). The plan describes the
 * complete version (`FR-UP-003`) and is rebuilt from frame zero when an interrupted upload restarts
 * (`FR-UP-016`).
 */
internal data class UploadFramePlan(
    /** The plaintext payload of every upload frame, in order (frame 0 first). */
    val frames: List<ByteArray>,
) {
    /** The number of upload frames the version is split into. */
    val frameCount: Int
        get() = frames.size

    /** The plaintext payload of the upload frame at the given position in the upload order. */
    fun frame(index: Int): ByteArray = frames[index]
}

/**
 * Splits one compressed version into the plaintext payloads of its upload frames (`FR-UP-003`,
 * `FR-UP-004`, `FR-UP-005`, `FR-UP-016`, `CFG-CL-011`).
 *
 * The configured frame limit applies to the whole frame: the four-byte length prefix, the external
 * header, the 64-byte Ed25519 signature, the 48 bytes of HPKE encapsulated key and AES-GCM tag, and
 * the plaintext payload (`CFG-CL-011`). The external header is bounded by encoding the template
 * with the widest `uint32` frame number, 4294967295, so the budget does not depend on the actual
 * frame number (`INT-NET-050`). The budget for one frame's plaintext is therefore
 * `maxFrameBytes - 4 - 64 - 48 - worstCaseHeaderBytes`.
 *
 * Frame zero carries the non-empty logical header followed by the first compressed bytes, and each
 * later frame carries only the next compressed bytes; every frame except the last is filled to the
 * budget (`FR-UP-004`, `FR-UP-005`). An empty compressed payload yields a single frame containing
 * only the logical header. The encoded logical header is bounded by `CFG-CL-046`.
 *
 * A limit whose budget does not exceed the logical header is a configuration error, because frame
 * zero must carry the header and the start of the payload together (`FR-UP-004`, `CFG-CL-011`).
 */
internal object UploadFramePlanBuilder {
    fun plan(
        headerTemplate: ExternalFrameHeader,
        logicalHeader: ByteArray,
        payload: CompressedPayload,
        maxFrameBytes: Int,
    ): UploadFramePlan {
        require(maxFrameBytes > 0) { "maximum frame size must be positive" }
        require(logicalHeader.isNotEmpty()) { "logical header must not be empty" }
        val budget = frameBudget(headerTemplate, maxFrameBytes)
        require(budget > logicalHeader.size.toLong()) {
            "configured maximum frame size cannot carry the logical header"
        }
        return UploadFramePlan(frames = buildFrames(logicalHeader, payload, budget.toInt()))
    }
}

private fun frameBudget(
    headerTemplate: ExternalFrameHeader,
    maxFrameBytes: Int,
): Long {
    val worstCaseHeaderBytes =
        ExternalFrameHeaderCodec
            .encode(headerTemplate.copy(frameNumber = WIDEST_FRAME_NUMBER))
            .size
    return maxFrameBytes.toLong() -
        LENGTH_PREFIX_BYTES -
        SIGNATURE_BYTES -
        ENCRYPTION_OVERHEAD_BYTES -
        worstCaseHeaderBytes
}

private fun buildFrames(
    logicalHeader: ByteArray,
    payload: CompressedPayload,
    frameBytes: Int,
): List<ByteArray> {
    val frames = mutableListOf<ByteArray>()
    var remaining = payload.totalBytes
    var blockIndex = 0
    var blockOffset = 0
    var frameIndex = 0
    while (remaining > 0L || frameIndex == FIRST_FRAME_INDEX) {
        val prefixBytes = if (frameIndex == FIRST_FRAME_INDEX) logicalHeader.size else 0
        val chunkBytes = minOf((frameBytes - prefixBytes).toLong(), remaining).toInt()
        val frame = ByteArray(prefixBytes + chunkBytes)
        if (frameIndex == FIRST_FRAME_INDEX) {
            logicalHeader.copyInto(frame)
        }
        var written = 0
        while (written < chunkBytes) {
            val block = payload.blocks[blockIndex]
            val copied = minOf(block.size - blockOffset, chunkBytes - written)
            block.copyInto(frame, prefixBytes + written, blockOffset, blockOffset + copied)
            written += copied
            blockOffset += copied
            if (blockOffset == block.size) {
                blockIndex += 1
                blockOffset = 0
            }
        }
        frames += frame
        remaining -= chunkBytes
        frameIndex += 1
    }
    return frames
}

private const val FIRST_FRAME_INDEX = 0

// Every frame starts with a four-byte length prefix, the external header and a 64-byte Ed25519
// signature before its encrypted payload (INT-NET-081).
private const val LENGTH_PREFIX_BYTES = 4
private const val SIGNATURE_BYTES = 64

// The encrypted payload adds a 32-byte HPKE encapsulated key and a 16-byte AES-GCM tag (INT-NET-024,
// INT-NET-081).
private const val HPKE_ENCAPSULATED_KEY_BYTES = 32
private const val GCM_TAG_BYTES = 16
private const val ENCRYPTION_OVERHEAD_BYTES = HPKE_ENCAPSULATED_KEY_BYTES + GCM_TAG_BYTES

// The widest uint32 frame number makes the encoded external header as large as it can be
// (INT-NET-050).
private const val WIDEST_FRAME_NUMBER = 0xFFFF_FFFFL
