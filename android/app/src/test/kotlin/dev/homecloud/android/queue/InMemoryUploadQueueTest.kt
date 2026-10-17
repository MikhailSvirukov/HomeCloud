package dev.homecloud.android.queue

import dev.homecloud.android.media.MediaVersionIdentity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class InMemoryUploadQueueTest {
    @Test
    fun enqueueAddsPendingEntryWithZeroAttempts() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val entry = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, identityA, SIZE_BYTES_A)

        assertEquals(UPLOAD_ID_A, entry.uploadId)
        assertEquals(SOURCE_ID_A, entry.sourceId)
        assertEquals(identityA, entry.identity)
        assertEquals(SIZE_BYTES_A, entry.sizeBytes)
        assertEquals(0, entry.attemptCount)
        assertEquals(UploadQueueState.PENDING, entry.state)
    }

    @Test
    fun enqueueSameUploadIdTwiceYieldsSingleEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val first = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, identityA, SIZE_BYTES_A)

        val second = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, otherIdentity, OTHER_SIZE_BYTES)

        assertEquals(first, second)
        assertEquals(1, queue.entries().size)
    }

    @Test
    fun enqueueSameIdentityWithDifferentUploadIdReturnsFirstEntryAndKeepsOneEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val first = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, identityA, SIZE_BYTES_A)

        val second = queue.enqueue(UPLOAD_ID_B, SOURCE_ID_A, identityA, OTHER_SIZE_BYTES)

        assertEquals(first, second)
        assertEquals(listOf(first), queue.entries())
        assertEquals(UPLOAD_ID_A, queue.findVersion(identityA)?.uploadId)
    }

    @Test
    fun changedIdentityForSameSourceIdCreatesSecondEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val unchanged = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, identityA, SIZE_BYTES_A)

        val changed = queue.enqueue(UPLOAD_ID_B, SOURCE_ID_A, otherIdentity, SIZE_BYTES_A)

        assertEquals(2, queue.entries().size)
        assertEquals(unchanged, queue.findVersion(identityA))
        assertEquals(changed, queue.findVersion(otherIdentity))
        assertEquals(PENDING_BEFORE_COMPLETION, queue.pendingCount())
    }

    @Test
    fun reEnqueueAfterMarkCompletedYieldsSingleCompletedEntryWithPreservedAttempts() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markInFlight(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)

        queue.markCompleted(UPLOAD_ID_A)
        val reEnqueued = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, identityA, SIZE_BYTES_A)

        assertEquals(1, queue.entries().size)
        assertEquals(UploadQueueState.COMPLETED, reEnqueued.state)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, reEnqueued.attemptCount)
    }

    @Test
    fun findVersionReturnsEntryInNonPendingState() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markCompleted(UPLOAD_ID_A)

        val tracked = queue.findVersion(identityA)

        assertEquals(UploadQueueState.COMPLETED, tracked?.state)
        assertEquals(UPLOAD_ID_A, tracked?.uploadId)
    }

    @Test
    fun findVersionReturnsNullForUnknownIdentity() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)

        assertNull(queue.findVersion(otherIdentity))
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
    fun requeueFailedMovesFailedEntryToPendingResetsAttemptsAndMakesItSelectable() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)
        assertNull(queue.nextPending())

        queue.requeueFailed()

        val entry = queue.entry(UPLOAD_ID_A)
        assertEquals(UploadQueueState.PENDING, entry.state)
        assertEquals(0, entry.attemptCount)
        assertEquals(UPLOAD_ID_A, queue.nextPending()?.uploadId)
    }

    @Test
    fun requeueFailedReturnsNumberOfReArmedEntries() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)
        queue.markFailed(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_B)
        queue.markFailed(UPLOAD_ID_B)

        val reArmed = queue.requeueFailed()

        assertEquals(RE_ARMED_ENTRY_COUNT, reArmed)
    }

    @Test
    fun requeueFailedLeavesPendingInFlightAndCompletedEntriesUnchanged() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)
        queue.enqueueUpload(UPLOAD_ID_B)
        queue.markFailed(UPLOAD_ID_B)
        queue.markInFlight(UPLOAD_ID_B)
        queue.enqueueUpload(UPLOAD_ID_C)
        queue.markInFlight(UPLOAD_ID_C)
        queue.markCompleted(UPLOAD_ID_C)

        val reArmed = queue.requeueFailed()

        assertEquals(0, reArmed)

        val pending = queue.entry(UPLOAD_ID_A)
        assertEquals(UploadQueueState.PENDING, pending.state)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, pending.attemptCount)

        val inFlight = queue.entry(UPLOAD_ID_B)
        assertEquals(UploadQueueState.IN_FLIGHT, inFlight.state)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, inFlight.attemptCount)

        val completed = queue.entry(UPLOAD_ID_C)
        assertEquals(UploadQueueState.COMPLETED, completed.state)
        assertEquals(0, completed.attemptCount)
    }

    @Test
    fun requeueFailedRestoresFullAttemptBudget() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        queue.enqueueUpload(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)
        queue.markFailed(UPLOAD_ID_A)

        queue.requeueFailed()
        queue.markFailed(UPLOAD_ID_A)

        val afterFirstFailure = queue.entry(UPLOAD_ID_A)
        assertEquals(ATTEMPTS_AFTER_ONE_FAILURE, afterFirstFailure.attemptCount)
        assertEquals(UploadQueueState.PENDING, afterFirstFailure.state)

        queue.markFailed(UPLOAD_ID_A)

        val afterSecondFailure = queue.entry(UPLOAD_ID_A)
        assertEquals(MAX_ATTEMPTS_TWO, afterSecondFailure.attemptCount)
        assertEquals(UploadQueueState.FAILED, afterSecondFailure.state)
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
    fun markOperationsWithUnknownUploadIdThrowIllegalArgumentException() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val inFlight =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markInFlight(UNKNOWN_UPLOAD_ID)
            }
        val completed =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markCompleted(UNKNOWN_UPLOAD_ID)
            }
        val failed =
            assertThrows(IllegalArgumentException::class.java) {
                queue.markFailed(UNKNOWN_UPLOAD_ID)
            }

        assertTrue(inFlight.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
        assertTrue(completed.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
        assertTrue(failed.message.orEmpty().contains(UNKNOWN_UPLOAD_ID))
    }

    @Test
    fun restoreMakesQueueTrackExactlyTheGivenEntries() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val snapshot =
            listOf(
                restoredEntry(UPLOAD_ID_A, UploadQueueState.PENDING),
                restoredEntry(UPLOAD_ID_B, UploadQueueState.COMPLETED),
            )

        queue.restore(snapshot)

        assertEquals(snapshot, queue.entries())
    }

    @Test
    fun restorePreservesStateAndAttemptCount() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        val failed = restoredEntry(UPLOAD_ID_A, UploadQueueState.FAILED, RESTORED_ATTEMPT_COUNT)
        val completed = restoredEntry(UPLOAD_ID_B, UploadQueueState.COMPLETED)

        queue.restore(listOf(failed, completed))

        val restoredFailed = queue.entry(UPLOAD_ID_A)
        assertEquals(UploadQueueState.FAILED, restoredFailed.state)
        assertEquals(RESTORED_ATTEMPT_COUNT, restoredFailed.attemptCount)

        val restoredCompleted = queue.entry(UPLOAD_ID_B)
        assertEquals(UploadQueueState.COMPLETED, restoredCompleted.state)
        assertEquals(0, restoredCompleted.attemptCount)
    }

    @Test
    fun afterRestoreNextPendingReturnsOldestRestoredPendingAndSkipsNonPending() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.restore(
            listOf(
                restoredEntry(UPLOAD_ID_A, UploadQueueState.COMPLETED),
                restoredEntry(UPLOAD_ID_B, UploadQueueState.PENDING),
                restoredEntry(UPLOAD_ID_C, UploadQueueState.FAILED),
                restoredEntry(UPLOAD_ID_D, UploadQueueState.PENDING),
            ),
        )

        assertEquals(UPLOAD_ID_B, queue.nextPending()?.uploadId)
    }

    @Test
    fun restoreRequeuesInFlightEntryToPendingAndReturnsItFromNextPending() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        queue.restore(listOf(restoredEntry(UPLOAD_ID_A, UploadQueueState.IN_FLIGHT)))

        assertEquals(UploadQueueState.PENDING, queue.entry(UPLOAD_ID_A).state)
        assertEquals(UPLOAD_ID_A, queue.nextPending()?.uploadId)
    }

    @Test
    fun restoreDoesNotRequeueCompletedOrFailedEntries() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        queue.restore(
            listOf(
                restoredEntry(UPLOAD_ID_A, UploadQueueState.COMPLETED),
                restoredEntry(UPLOAD_ID_B, UploadQueueState.FAILED, RESTORED_ATTEMPT_COUNT),
            ),
        )

        assertEquals(UploadQueueState.COMPLETED, queue.entry(UPLOAD_ID_A).state)
        assertEquals(UploadQueueState.FAILED, queue.entry(UPLOAD_ID_B).state)
        assertNull(queue.nextPending())
        assertEquals(0, queue.pendingCount())
    }

    @Test
    fun restoreRequeuesInFlightEntryWithItsIdentityAndAttemptCount() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val inFlight = restoredEntry(UPLOAD_ID_B, UploadQueueState.IN_FLIGHT, RESTORED_ATTEMPT_COUNT)

        queue.restore(listOf(inFlight))

        assertEquals(inFlight.copy(state = UploadQueueState.PENDING), queue.entry(UPLOAD_ID_B))
    }

    @Test
    fun restoreReplacesPreviouslyTrackedEntries() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)
        val restored = restoredEntry(UPLOAD_ID_B, UploadQueueState.PENDING)

        queue.restore(listOf(restored))

        assertEquals(listOf(restored), queue.entries())
    }

    @Test
    fun restoreEmptyListLeavesQueueEmpty() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        queue.enqueueUpload(UPLOAD_ID_A)

        queue.restore(emptyList())

        assertTrue(queue.entries().isEmpty())
        assertNull(queue.nextPending())
        assertEquals(0, queue.pendingCount())
    }

    @Test
    fun restoreWithDuplicateUploadIdKeepsLaterEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        val earlier = restoredEntry(UPLOAD_ID_A, UploadQueueState.PENDING)
        val later = restoredEntry(UPLOAD_ID_A, UploadQueueState.FAILED, RESTORED_ATTEMPT_COUNT)

        queue.restore(listOf(earlier, later))

        assertEquals(listOf(later), queue.entries())
    }

    @Test
    fun enqueueRestoredUploadIdReturnsRestoredEntryUnchangedWithoutDuplicate() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS_TWO)
        val restored = restoredEntry(UPLOAD_ID_A, UploadQueueState.FAILED, RESTORED_ATTEMPT_COUNT)
        queue.restore(listOf(restored))

        val returned = queue.enqueue(UPLOAD_ID_A, SOURCE_ID_A, otherIdentity, OTHER_SIZE_BYTES)

        assertEquals(restored, returned)
        assertEquals(1, queue.entries().size)
    }

    @Test
    fun findVersionFindsRestoredEntry() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val restored = restoredEntry(UPLOAD_ID_A, UploadQueueState.FAILED, RESTORED_ATTEMPT_COUNT)
        queue.restore(listOf(restored))

        assertEquals(restored, queue.findVersion(identityA))
    }

    private fun restoredEntry(
        uploadId: String,
        state: UploadQueueState,
        attemptCount: Int = 0,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = SOURCE_ID_A,
            identity = identityOf(uploadId),
            sizeBytes = SIZE_BYTES_A,
            attemptCount = attemptCount,
            state = state,
        )

    private fun InMemoryUploadQueue.enqueueUpload(uploadId: String): UploadQueueEntry =
        enqueue(uploadId, SOURCE_ID_A, identityOf(uploadId), SIZE_BYTES_A)

    private fun InMemoryUploadQueue.entry(uploadId: String): UploadQueueEntry = entries().first { it.uploadId == uploadId }

    private fun identityOf(uploadId: String): MediaVersionIdentity =
        when (uploadId) {
            UPLOAD_ID_B -> identityB
            UPLOAD_ID_C -> identityC
            UPLOAD_ID_D -> identityD
            else -> identityA
        }

    private companion object {
        const val UPLOAD_ID_A = "upload-a"
        const val UPLOAD_ID_B = "upload-b"
        const val UPLOAD_ID_C = "upload-c"
        const val UPLOAD_ID_D = "upload-d"
        const val UNKNOWN_UPLOAD_ID = "upload-unknown"

        const val SOURCE_ID_A = 1L
        const val SIZE_BYTES_A = 1024L
        const val OTHER_SIZE_BYTES = 2048L

        const val CONTENT_SHA256_A = "sha256-a"
        const val CONTENT_SHA256_B = "sha256-b"
        const val ORIGINAL_NAME_A = "photo-a.jpg"
        const val ORIGINAL_NAME_B = "photo-b.jpg"
        const val TRACKED_TIME_A = 1_700_000_000L
        const val TRACKED_TIME_B = 1_700_000_100L

        val identityA = MediaVersionIdentity(CONTENT_SHA256_A, ORIGINAL_NAME_A, TRACKED_TIME_A)
        val identityB = MediaVersionIdentity(CONTENT_SHA256_B, ORIGINAL_NAME_A, TRACKED_TIME_A)
        val identityC = MediaVersionIdentity(CONTENT_SHA256_A, ORIGINAL_NAME_B, TRACKED_TIME_A)
        val identityD = MediaVersionIdentity(CONTENT_SHA256_A, ORIGINAL_NAME_A, TRACKED_TIME_B)
        val otherIdentity = MediaVersionIdentity(CONTENT_SHA256_B, ORIGINAL_NAME_B, TRACKED_TIME_B)

        const val MAX_ATTEMPTS = 3
        const val MAX_ATTEMPTS_TWO = 2
        const val SINGLE_ATTEMPT = 1

        const val ATTEMPTS_AFTER_ONE_FAILURE = 1
        const val RESTORED_ATTEMPT_COUNT = 2
        const val PENDING_BEFORE_COMPLETION = 2
        const val RE_ARMED_ENTRY_COUNT = 2
    }
}
