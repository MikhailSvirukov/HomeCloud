package dev.homecloud.android.upload

import dev.homecloud.android.payload.CompressedPayload
import dev.homecloud.android.protocol.ExternalFrameHeader
import dev.homecloud.android.protocol.ExternalFrameHeaderCodec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class UploadFramePlanTest {
    @Test
    fun `a payload within the frame budget yields one frame with the header`() {
        val payload = payload(PAYLOAD_WITHIN_BUDGET)

        val plan = plan(payload)

        assertEquals(1, plan.frameCount)
        assertArrayEquals(LOGICAL_HEADER + payloadBytes(payload), plan.frame(0))
    }

    @Test
    fun `a payload exactly filling the remaining budget yields one frame`() {
        val payload = payload(budget() - LOGICAL_HEADER.size)

        val plan = plan(payload)

        assertEquals(1, plan.frameCount)
        assertEquals(budget(), plan.frame(0).size)
    }

    @Test
    fun `a payload above the frame budget spans the expected number of filled frames`() {
        val payload = payload(47, 53, 60, 40)

        val plan = plan(payload)

        val bound = budget()
        assertEquals(expectedFrameCount(payload.totalBytes.toInt(), bound), plan.frameCount)
        plan.frames.forEach { frame -> assertTrue(frame.size <= bound) }
        plan.frames.dropLast(1).forEach { frame -> assertEquals(bound, frame.size) }
    }

    @Test
    fun `frame zero starts with the header and later frames carry only payload`() {
        val payload = payload(47, 53, 60, 40)

        val plan = plan(payload)

        assertArrayEquals(LOGICAL_HEADER, plan.frame(0).copyOfRange(0, LOGICAL_HEADER.size))
        val bytes = payloadBytes(payload)
        var offset = plan.frame(0).size - LOGICAL_HEADER.size
        for (index in 1 until plan.frameCount) {
            val frame = plan.frame(index)
            assertArrayEquals(bytes.copyOfRange(offset, offset + frame.size), frame)
            offset += frame.size
        }
    }

    @Test
    fun `concatenating the payload after the header with later frames reproduces the payload`() {
        val payload = payload(47, 53, 60, 40)

        val plan = plan(payload)
        val firstFrame = plan.frame(0)

        val reconstructed =
            plan.frames.drop(1).fold(firstFrame.copyOfRange(LOGICAL_HEADER.size, firstFrame.size)) { bytes, frame ->
                bytes + frame
            }

        assertArrayEquals(payloadBytes(payload), reconstructed)
    }

    @Test
    fun `an empty payload yields one header-only frame`() {
        val plan = plan(payload())

        assertEquals(1, plan.frameCount)
        assertArrayEquals(LOGICAL_HEADER, plan.frame(0))
    }

    @Test
    fun `a limit that cannot carry the logical header is rejected`() {
        val maxFrameBytes =
            worstCaseHeaderBytes() +
                SIGNATURE_BYTES +
                ENCRYPTION_OVERHEAD_BYTES +
                LOGICAL_HEADER.size

        val error =
            assertThrows(IllegalArgumentException::class.java) {
                plan(payload(1), maxFrameBytes = maxFrameBytes)
            }

        assertTrue(error.message.orEmpty().contains("cannot carry the logical header"))
    }

    @Test
    fun `an empty logical header is rejected`() {
        val error =
            assertThrows(IllegalArgumentException::class.java) {
                plan(payload(1), header = ByteArray(0))
            }

        assertTrue(error.message.orEmpty().contains("logical header"))
    }

    @Test
    fun `a non-positive frame limit is rejected`() {
        val error =
            assertThrows(IllegalArgumentException::class.java) {
                plan(payload(1), maxFrameBytes = 0)
            }

        assertTrue(error.message.orEmpty().contains("positive"))
    }

    @Test
    fun `two builds from the same input produce the same frames`() {
        val payload = payload(47, 53, 60, 40)

        val first = plan(payload)
        val second = plan(payload)

        assertEquals(first.frameCount, second.frameCount)
        for (index in 0 until first.frameCount) {
            assertArrayEquals(first.frame(index), second.frame(index))
        }
    }

    @Test
    fun `frame rejects indices outside the plan`() {
        val plan = plan(payload())

        assertThrows(IndexOutOfBoundsException::class.java) { plan.frame(-1) }
        assertThrows(IndexOutOfBoundsException::class.java) { plan.frame(plan.frameCount) }
    }

    private fun plan(
        payload: CompressedPayload,
        header: ByteArray = LOGICAL_HEADER,
        maxFrameBytes: Int = MAX_FRAME_BYTES,
    ): UploadFramePlan = UploadFramePlanBuilder.plan(HEADER_TEMPLATE, header, payload, maxFrameBytes)

    private fun budget(): Int =
        MAX_FRAME_BYTES -
            SIGNATURE_BYTES -
            ENCRYPTION_OVERHEAD_BYTES -
            worstCaseHeaderBytes()

    private fun worstCaseHeaderBytes(): Int = ExternalFrameHeaderCodec.encode(HEADER_TEMPLATE.copy(frameNumber = WIDEST_FRAME_NUMBER)).size

    private fun expectedFrameCount(
        payloadBytes: Int,
        frameBudget: Int,
    ): Int {
        val remaining = payloadBytes - (frameBudget - LOGICAL_HEADER.size)
        return if (remaining <= 0) 1 else 1 + (remaining + frameBudget - 1) / frameBudget
    }

    private fun payloadBytes(payload: CompressedPayload): ByteArray = payload.blocks.fold(ByteArray(0)) { bytes, block -> bytes + block }

    private fun payload(vararg blockSizes: Int): CompressedPayload =
        CompressedPayload(
            blockSizes.mapIndexed { blockIndex, size ->
                ByteArray(size) { offset -> (blockIndex * BLOCK_FILL_STEP + offset).toByte() }
            },
        )
}

private val LOGICAL_HEADER = byteArrayOf(0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16)

private val HEADER_TEMPLATE =
    ExternalFrameHeader(
        senderId = ByteArray(16) { offset -> offset.toByte() },
        requestId = ByteArray(16) { offset -> (offset + 1).toByte() },
        frameNumber = 0,
        uploadId = ByteArray(16) { offset -> (offset + 2).toByte() },
        responderLabel = null,
        bootstrapSigningPublicKey = null,
    )

private const val MAX_FRAME_BYTES = 256
private const val PAYLOAD_WITHIN_BUDGET = 40
private const val BLOCK_FILL_STEP = 31
private const val SIGNATURE_BYTES = 64
private const val HPKE_ENCAPSULATED_KEY_BYTES = 32
private const val GCM_TAG_BYTES = 16
private const val ENCRYPTION_OVERHEAD_BYTES = HPKE_ENCAPSULATED_KEY_BYTES + GCM_TAG_BYTES
private const val WIDEST_FRAME_NUMBER = 0xFFFF_FFFFL
