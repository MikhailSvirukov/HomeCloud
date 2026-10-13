package dev.picbackup.android.queue

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class InMemoryUploadQueueTest {

    @Test
    fun enqueueAddsPendingEntryWithZeroAttempts() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val entry = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, SIZE_BYTES_A, DATE_MODIFIED_A)

        assertEquals(UPLOAD_ID_A, entry.uploadId)
        assertEquals(SOURCE_ID_A, entry.sourceId)
        assertEquals(SIZE_BYTES_A, entry.sizeBytes)
        assertEquals(DATE_MODIFIED_A, entry.dateModifiedEpochSeconds)
        assertEquals(0, entry.attemptCount)
        assertEquals(UploadQueueState.PENDING, entry.state)
    }

    @Test
    fun enqueueSameUploadIdTwiceYieldsSingleEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val first = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, SIZE_BYTES_A, DATE_MODIFIED_A)

        val second = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, OTHER_SIZE_BYTES, OTHER_DATE_MODIFIED)

        assertEquals(first, second)
        assertEquals(1, queue.entries().size)
    }

    @Test
    fun reEnqueueAfterMarkCompletedYieldsSingleCompletedEntryWithPreservedAttempts() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markInFlight(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)

        queue.markCompleted(UPLOAD_ID_A)
        val reEnqueued = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, SIZE_BYTES_A, DATE_MODIFIED_A)

        assertEquals(1, queue.entries().size)
        assertEquals(UploadQueueState.COMPLETED, reEnqueued.state)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, reEnqueued.attemptCount)
    }

    @Test
    fun nextPendingReturnsEntriesInInsertionOrder() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)
        queue.enqueueUpload(UPLOAD_ID_C)

        assertEquals(UPLOAD_ID_A, queue.nextPending()?.uploadId)
        queue.markInFlight(UPLOAD_ID_A)
        assertEquals(UPLOAD_ID_B, queue.nextPending()?.uploadId)
        queue.markInFlight(UPLOAD_ID_B)
        assertEquals(UPLOAD_ID_C, queue.nextPending()?.uploadId)
    }

    @Test
    fun nextPendingSkipsEntriesThatAreNotPending() {
        val queue = InMemoryUploadQueue(SINGLE_ATTEMPT)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)
        queue.enqueueUpload(UPLOAD_ID_C)
        queue.markCompleted(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_B)
        queue.markInFlight(UPLOAD_ID_C)

        assertNull(queue.nextPending())

        queue.enqueueUpload(UPLOAD_ID_D)

        assertEquals(UPLOAD_ID_D, queue.nextPending()?.uploadId)
    }

    @Test
    fun nextPendingReturnsNullWhenEmpty() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        assertNull(queue.nextPending())
    }

    @Test
    fun markInFlightMovesEntryToInFlightAndRemovesItFromNextPending() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)

        queue.markInFlight(UPLOAD_ID_A)

        assertEquals(UploadQueueState.IN_FLIGHT, queue.entry(UPLOAD_ID_A).state)
        assertEquals(UPLOAD_ID_B, queue.nextPending()?.uploadId)
    }

    @Test
    fun markFailedIncrementsAttemptsAndKeepsEntryPendingBelowMaxAttempts() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)

        queue.markFailed(UPLOAD_ID_A)

        val entry = queue.entry(UPLOAD_ID_A)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, entry.attemptCount)
        assertEquals(UploadQueueState.PENDING, entry.state)
        assertEquals(UPLOAD_ID_A, queue.nextPending()?.uploadId)
    }

    @Test
    fun markFailedMovesEntryToFailedAtMaxAttemptsAndRemovesItFromNextPending() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        queue.enqueueUpload(UPLOAD_ID_A)

        queue.markFailed(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)

        val entry = queue.entry(UPLOAD_ID_A)
        assertEquals(MAX_ATTEMPTS_TWO, entry.attemptCount)
        assertEquals(UploadQueueState.FAILED, entry.state)
        assertNull(queue.nextPending())
    }

    @Test
    fun markCompletedSetsCompletedAndReducesPendingCount() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)
        assertEquals(PENDING_BEFORE_COMPLETION, queue.pendingCount())

        queue.markCompleted(UPLOAD_ID_A)

        assertEquals(UploadQueueState.COMPLETED, queue.entry(UPLOAD_ID_A).state)
        assertEquals(1, queue.pendingCount())
    }

    @Test
    fun markInFlightWithUnknownUploadIdThrowsIllegalArgumentException() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val thrown =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markInFlight(UNKNOWN_UPLOAD_ID)
            }

        assertTrue(thrown.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
    }

    @Test
    fun markCompletedWithUnknownUploadIdThrowsIllegalArgumentException() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val thrown =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markCompleted(UNKNOWN_UPLOAD_ID)
            }

        assertTrue(thrown.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
    }

    @Test
    fun markFailedWithUnknownUploadIdThrowsIllegalArgumentException() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val thrown =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markFailed(UNKNOWN_UPLOAD_ID)
            }

        assertTrue(thrown.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
    }

    @Test
    fun constructorRejectsZeroMaxAttempts() {
        assertThrows(IllegalArgumentException::class.java) { InMemoryUploadQueue(0) }
    }

    @Test
    fun constructorRejectsNegativeMaxAttempts() {
        assertThrows(IllegalArgumentException::class.java) { InMemoryUploadQueue(-1) }
    }

    private fun InMemoryUploadQueue.enqueueUpload(uploadId: String): UploadQueueEntry =
        enqueue(uploadId, SOURCE_ID_A, SIZE_BYTES_A, DATE_MODIFIED_A)

    private fun InMemoryUploadQueue.entry(uploadId: String): UploadQueueEntry =
        entries().first { it.uploadId == uploadId }

    private companion object {
        const val UPLOAD_ID_A = "upload-a"
        const val UPLOAD_ID_B = "upload-b"
        const val UPLOAD_ID_C = "upload-c"
        const val UPLOAD_ID_D = "upload-d"
        const val UNKNOWN_UPLOAD_ID = "upload-unknown"

        const val SOURCE_ID_A = 1L
        const val SIZE_BYTES_A = 1024L
        const val DATE_MODIFIED_A = 1_700_000_000L
        const val OTHER_SIZE_BYTES = 2048L
        const val OTHER_DATE_MODIFIED = 1_700_000_100L

        const val MAX_ATTEMPTS = 3
        const val MAX_ATTEMPTS_TWO = 2
        const val SINGLE_ATTEMPT = 1

        const val ATTEMPTS_AFTER_ONE_FAILURE = 1
        const val PENDING_BEFORE_COMPLETION = 2
    }
}
