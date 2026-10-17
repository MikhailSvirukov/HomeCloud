package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaKind
import dev.homecloud.android.media.MediaVersionCandidate
import dev.homecloud.android.queue.InMemoryUploadQueue
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.testsupport.FakeMediaVersionScanner
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
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
        val planner = MediaBackupPlanner(scanner, queue, NON_BINDING_CAPACITY)

        val result = planner.plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 2,
                newlyEnqueued = 2,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
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
        MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue, NON_BINDING_CAPACITY).plan()
        val uploadId = queue.entries().single().uploadId

        val result = MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue, NON_BINDING_CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 1,
                newlyEnqueued = 0,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
        assertEquals(uploadId, queue.entries().single().uploadId)
    }

    @Test
    fun `plan enqueues a changed version of the same source under a new upload id`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        val original = MediaTestData.candidate(id = CANDIDATE_ID, kind = MediaKind.IMAGE)
        MediaBackupPlanner(FakeMediaVersionScanner(listOf(original)), queue, NON_BINDING_CAPACITY).plan()
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

        val result = MediaBackupPlanner(FakeMediaVersionScanner(listOf(changed)), queue, NON_BINDING_CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 1,
                newlyEnqueued = 1,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
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

        val result = MediaBackupPlanner(scanner, queue, NON_BINDING_CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 2,
                newlyEnqueued = 1,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
        assertEquals(1, queue.entries().size)
    }

    @Test
    fun `plan over an empty scan enqueues nothing`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val result =
            MediaBackupPlanner(FakeMediaVersionScanner(emptyList()), queue, NON_BINDING_CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 0,
                newlyEnqueued = 0,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
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

        MediaBackupPlanner(FakeMediaVersionScanner(listOf(candidate)), queue, NON_BINDING_CAPACITY).plan()

        val entry = queue.entries().single()
        assertEquals(CANDIDATE_ID, entry.sourceId)
        assertEquals(CANDIDATE_SIZE_BYTES, entry.sizeBytes)
        assertEquals(candidate.identity, entry.identity)
    }

    @Test
    fun `plan rejects a configured queue size below one`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        assertThrows(IllegalArgumentException::class.java) {
            MediaBackupPlanner(FakeMediaVersionScanner(emptyList()), queue, INVALID_CAPACITY)
        }
    }

    @Test
    fun `plan enqueues exactly the configured capacity and rejects the candidate beyond it`() {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)

        val result =
            MediaBackupPlanner(
                FakeMediaVersionScanner(untrackedCandidates(count = CAPACITY + 1)),
                queue,
                CAPACITY,
            ).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = CAPACITY + 1,
                newlyEnqueued = CAPACITY,
                rejectedByCapacity = 1,
            ),
            result,
        )
        assertEquals(CAPACITY, queue.entries().size)
    }

    @Test
    fun `plan over a full queue enqueues nothing, keeps the accepted entries and rejects every candidate`() {
        val accepted = untrackedCandidates(count = CAPACITY)
        val queue = queueHolding(accepted)
        val acceptedEntries = queue.entries()
        val incoming =
            untrackedCandidates(count = INCOMING_CANDIDATE_COUNT, firstId = CAPACITY + 1L)

        val result = MediaBackupPlanner(FakeMediaVersionScanner(incoming), queue, CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = INCOMING_CANDIDATE_COUNT,
                newlyEnqueued = 0,
                rejectedByCapacity = INCOMING_CANDIDATE_COUNT,
            ),
            result,
        )
        assertEquals(acceptedEntries, queue.entries())
    }

    @Test
    fun `plan does not count a tracked candidate as rejected when the queue is full`() {
        val tracked = MediaTestData.candidate(id = TRACKED_ID, kind = MediaKind.IMAGE)
        val queue =
            queueHolding(
                listOf(tracked) +
                    untrackedCandidates(count = CAPACITY - 1, firstId = TRACKED_ID + 1L),
            )

        val result = MediaBackupPlanner(FakeMediaVersionScanner(listOf(tracked)), queue, CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 1,
                newlyEnqueued = 0,
                rejectedByCapacity = NO_CAPACITY_REJECTIONS,
            ),
            result,
        )
    }

    @Test
    fun `plan counts every discovered candidate as enqueued, rejected, tracked or repeated`() {
        val tracked = MediaTestData.candidate(id = TRACKED_ID, kind = MediaKind.IMAGE)
        val queue = queueHolding(listOf(tracked))
        val repeatedIdentity = MediaTestData.versionIdentity(sourceId = REPEATED_ID)
        val candidates =
            listOf(
                tracked,
                MediaTestData.candidate(id = REPEATED_ID, kind = MediaKind.IMAGE, identity = repeatedIdentity),
                MediaTestData.candidate(id = REPEATED_ID + 1L, kind = MediaKind.VIDEO, identity = repeatedIdentity),
                MediaTestData.candidate(id = REPEATED_ID + 2L, kind = MediaKind.IMAGE),
                MediaTestData.candidate(id = REPEATED_ID + 3L, kind = MediaKind.VIDEO),
            )

        val result = MediaBackupPlanner(FakeMediaVersionScanner(candidates), queue, CAPACITY).plan()

        assertEquals(
            PlanResult(
                discoveredCandidates = 5,
                newlyEnqueued = 1,
                rejectedByCapacity = 2,
            ),
            result,
        )
        assertEquals(
            result.discoveredCandidates,
            result.newlyEnqueued +
                result.rejectedByCapacity +
                ALREADY_TRACKED_COUNT +
                REPEATED_IN_SCAN_COUNT,
        )
        assertEquals(CAPACITY, queue.entries().size)
    }

    private fun untrackedCandidates(
        count: Int,
        firstId: Long = FIRST_UNTRACKED_ID,
    ): List<MediaVersionCandidate> =
        (0 until count).map { offset ->
            MediaTestData.candidate(id = firstId + offset, kind = MediaKind.IMAGE)
        }

    private fun queueHolding(candidates: List<MediaVersionCandidate>): InMemoryUploadQueue {
        val queue = InMemoryUploadQueue(MAX_ATTEMPTS)
        candidates.forEach { candidate ->
            queue.enqueue(
                uploadId = UPLOAD_ID_PREFIX + candidate.item.id,
                sourceId = candidate.item.id,
                identity = candidate.identity,
                sizeBytes = candidate.item.sizeBytes,
            )
        }
        return queue
    }

    private companion object {
        const val CANDIDATE_ID = 42L
        const val TRACKED_ID = 1L
        const val REPEATED_ID = 2L
        const val FIRST_UNTRACKED_ID = 1L
        const val CANDIDATE_SIZE_BYTES = 4_096L
        const val CHANGED_CONTENT_SHA256 = "sha256-changed"
        const val UPLOAD_ID_PREFIX = "upload-"

        const val MAX_ATTEMPTS = 3
        const val SCAN_ONCE = 1
        const val NO_CAPACITY_REJECTIONS = 0

        const val CAPACITY = 2
        const val INVALID_CAPACITY = 0
        const val NON_BINDING_CAPACITY = 100
        const val INCOMING_CANDIDATE_COUNT = 2
        const val ALREADY_TRACKED_COUNT = 1
        const val REPEATED_IN_SCAN_COUNT = 1
    }
}
