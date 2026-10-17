package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaKind
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.testsupport.FakeMediaVersionScanner
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaBackupPlannerTest {
    @Test
    fun `plan enqueues every scanned candidate once and reports the counts`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val scanner =
            FakeMediaVersionScanner(
                listOf(
                    MediaTestData.candidate(id = 1L, kind = MediaKind.IMAGE),
                    MediaTestData.candidate(id = 2L, kind = MediaKind.VIDEO),
                ),
            )
        val planner = MediaBackupPlanner(scanner, queue)

        val result = planner.plan()

        assertEquals(PlanResult(discoveredCandidates = 2, newlyEnqueued = 2), result)
        assertEquals(2, queue.entries().size)
        assertEquals(
            listOf(UploadQueueState.PENDING, UploadQueueState.PENDING),
            queue.entries().map { entry -> entry.state },
        )
        assertEquals(SCAN_ONCE, scanner.scanCount)
    }

    @Test
    fun `plan leaves an unchanged candidate alone and keeps its upload id`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val candidate = MediaTestData.candidate(id = CANDIDATE_ID, kind = MediaKind.IMAGE)
        MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue).plan()
        val uploadId = queue.entries().single().uploadId

        val result = MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue).plan()

        assertEquals(PlanResult(discoveredCandidates = 1, newlyEnqueued = 0), result)
        assertEquals(uploadId, queue.entries().single().uploadId)
    }

    @Test
    fun `plan enqueues a changed version of the same source under a new upload id`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val original = MediaTestData.candidate(id = CANDIDATE_ID, kind = MediaKind.IMAGE)
        MediaBackupPlanner(FakeMediaVersionScanner(listOf(original)), queue).plan()
        val originalUploadId = queue.entries().single().uploadId
        val changed =
            MediaTestData.candidate(
                id = CANDIDATE_ID,
                kind = MediaKind.IMAGE,
                identity =
                    MediaTestData.versionIdentity(
                        sourceId = CANDIDATE_ID,
                        contentSha256 = CHANGED_CONTENT_SHA256,
                    ),
            )

        val result = MediaBackupPlanner(FakeMediaVersionScanner(listOf(changed)), queue).plan()

        assertEquals(PlanResult(discoveredCandidates = 1, newlyEnqueued = 1), result)
        val entries = queue.entries()
        assertEquals(2, entries.size)
        assertEquals(original.identity, entries.first().identity)
        assertEquals(changed.identity, entries.last().identity)
        assertEquals(1, entries.map { entry -> entry.sourceId }.distinct().size)
        assertNotEquals(originalUploadId, entries.last().uploadId)
    }

    @Test
    fun `plan enqueues one entry when two candidates share a version identity`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val sharedIdentity = MediaTestData.versionIdentity(sourceId = CANDIDATE_ID)
        val scanner =
            FakeMediaVersionScanner(
                listOf(
                    MediaTestData.candidate(id = 1L, kind = MediaKind.IMAGE, identity = sharedIdentity),
                    MediaTestData.candidate(id = 2L, kind = MediaKind.VIDEO, identity = sharedIdentity),
                ),
            )

        val result = MediaBackupPlanner(scanner, queue).plan()

        assertEquals(PlanResult(discoveredCandidates = 2, newlyEnqueued = 1), result)
        assertEquals(1, queue.entries().size)
    }

    @Test
    fun `plan over an empty scan enqueues nothing`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val result = MediaBackupPlanner(FakeMediaVersionScanner(emptyList()), queue).plan()

        assertEquals(PlanResult(discoveredCandidates = 0, newlyEnqueued = 0), result)
        assertTrue(queue.entries().isEmpty())
    }

    @Test
    fun `plan records the scanned candidate's source id, size and identity`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val candidate =
            MediaTestData.candidate(
                id = CANDIDATE_ID,
                kind = MediaKind.IMAGE,
                sizeBytes = CANDIDATE_SIZE_BYTES,
            )

        MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue).plan()

        val entry = queue.entries().single()
        assertEquals(CANDIDATE_ID, entry.sourceId)
        assertEquals(CANDIDATE_SIZE_BYTES, entry.sizeBytes)
        assertEquals(candidate.identity, entry.identity)
    }

    private companion object {
        const val CANDIDATE_ID = 42L
        const val CANDIDATE_SIZE_BYTES = 4_096L
        const val CHANGED_CONTENT_SHA256 = "sha256-changed"

        const val MAX_ATTEMPTS = 3
        const val SCAN_ONCE = 1
    }
}
