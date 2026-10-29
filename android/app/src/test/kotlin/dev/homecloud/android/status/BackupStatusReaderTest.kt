package dev.homecloud.android.status

import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BackupStatusReaderTest {
    @Test
    fun `statusOf without a recorded cycle keeps the supplied state and reports no completion time, outcome, or error`() {
        val status =
            BackupStatusReader.statusOf(
                entries = emptyList(),
                last = null,
                state = CycleState.SCANNING,
            )

        assertEquals(CycleState.SCANNING, status.state)
        assertNull(status.lastCompletedAtMillis)
        assertNull(status.lastOutcome)
        assertNull(status.lastError)
    }

    @Test
    fun `statusOf counts only pending and in-flight entries when the queue mixes every state`() {
        val status =
            BackupStatusReader.statusOf(
                entries =
                    listOf(
                        entry(sourceId = PENDING_SOURCE_ID, state = UploadQueueState.PENDING),
                        entry(sourceId = IN_FLIGHT_SOURCE_ID, state = UploadQueueState.IN_FLIGHT),
                        entry(sourceId = COMPLETED_SOURCE_ID, state = UploadQueueState.COMPLETED),
                        entry(sourceId = FAILED_SOURCE_ID, state = UploadQueueState.FAILED),
                    ),
                last = null,
                state = CycleState.IDLE,
            )

        assertEquals(EXPECTED_PENDING_VERSIONS, status.pendingVersions)
    }

    @Test
    fun `statusOf with a successful record reports completed, success, and no error`() {
        val status =
            BackupStatusReader.statusOf(
                entries = emptyList(),
                last =
                    LastCycleRecord(
                        completedAtMillis = COMPLETED_AT_MILLIS,
                        outcome = CycleOutcome.SUCCESS,
                        error = null,
                    ),
                state = CycleState.COMPLETED,
            )

        assertEquals(CycleState.COMPLETED, status.state)
        assertEquals(COMPLETED_AT_MILLIS, status.lastCompletedAtMillis)
        assertEquals(CycleOutcome.SUCCESS, status.lastOutcome)
        assertNull(status.lastError)
    }

    @Test
    fun `statusOf with a failed record reports failed, failure, and the recorded category`() {
        val status =
            BackupStatusReader.statusOf(
                entries = emptyList(),
                last =
                    LastCycleRecord(
                        completedAtMillis = COMPLETED_AT_MILLIS,
                        outcome = CycleOutcome.FAILURE,
                        error = LastErrorCategory.QUEUE_UNAVAILABLE,
                    ),
                state = CycleState.FAILED,
            )

        assertEquals(CycleState.FAILED, status.state)
        assertEquals(COMPLETED_AT_MILLIS, status.lastCompletedAtMillis)
        assertEquals(CycleOutcome.FAILURE, status.lastOutcome)
        assertEquals(LastErrorCategory.QUEUE_UNAVAILABLE, status.lastError)
    }

    @Test
    fun `statusOf uses the supplied scanning state when a record exists`() {
        val status =
            BackupStatusReader.statusOf(
                entries = emptyList(),
                last = successRecord(),
                state = CycleState.SCANNING,
            )

        assertEquals(CycleState.SCANNING, status.state)
    }

    @Test
    fun `statusOf uses the supplied uploading state when a record exists`() {
        val status =
            BackupStatusReader.statusOf(
                entries = emptyList(),
                last = successRecord(),
                state = CycleState.UPLOADING,
            )

        assertEquals(CycleState.UPLOADING, status.state)
    }

    private fun successRecord(): LastCycleRecord =
        LastCycleRecord(
            completedAtMillis = COMPLETED_AT_MILLIS,
            outcome = CycleOutcome.SUCCESS,
            error = null,
        )

    private fun entry(
        sourceId: Long,
        state: UploadQueueState,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = UPLOAD_ID_PREFIX + sourceId,
            sourceId = sourceId,
            identity = MediaTestData.versionIdentity(sourceId = sourceId),
            sizeBytes = MediaTestData.SIZE_BYTES,
            attemptCount = 0,
            state = state,
        )

    private companion object {
        const val COMPLETED_AT_MILLIS = 1_700_000_000_000L
        const val UPLOAD_ID_PREFIX = "upload-"

        const val PENDING_SOURCE_ID = 1L
        const val IN_FLIGHT_SOURCE_ID = 2L
        const val COMPLETED_SOURCE_ID = 3L
        const val FAILED_SOURCE_ID = 4L
        const val EXPECTED_PENDING_VERSIONS = 2
    }
}
