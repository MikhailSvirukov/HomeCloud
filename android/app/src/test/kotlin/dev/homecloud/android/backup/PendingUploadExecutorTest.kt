package dev.homecloud.android.backup

import dev.homecloud.android.media.MediaVersionIdentity
import dev.homecloud.android.payload.CompressedPayload
import dev.homecloud.android.protocol.TopologyNode
import dev.homecloud.android.queue.UploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.queue.UploadQueueState
import dev.homecloud.android.topology.TopologyException
import dev.homecloud.android.upload.UploadException
import dev.homecloud.android.upload.UploadMetadata
import dev.homecloud.android.upload.UploadRequest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * Asserts the pending-entry execution of `FR-AND-017`, `FR-AND-027`, `FR-AND-030`, `FR-AND-052`,
 * `FR-UP-016` and `FR-UP-021`.
 */
class PendingUploadExecutorTest {
    private val queuedEntry = newEntry()
    private val preparedUpload = newPreparedUpload()
    private val firstNode = node(label = FIRST_LABEL, host = FIRST_HOST, port = FIRST_PORT)
    private val secondNode = node(label = SECOND_LABEL, host = SECOND_HOST, port = SECOND_PORT)
    private val thirdNode = node(label = THIRD_LABEL, host = THIRD_HOST, port = THIRD_PORT)

    @Test
    fun `first node success marks in flight then completed and uploads once`() {
        val queue = RecordingUploadQueue(queuedEntry)
        var preparationCount = 0
        var topologyCount = 0
        val uploads = mutableListOf<UploadRequest>()
        val executor =
            executor(
                queue = queue,
                prepare = { _, _ ->
                    preparationCount += 1
                    preparedUpload
                },
                fetchTopology = { _, _, _, _ ->
                    topologyCount += 1
                    listOf(firstNode)
                },
                upload = { request ->
                    uploads += request
                    preparedUpload.uploadId
                },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.COMPLETED, outcome)
        assertEquals(1, preparationCount)
        assertEquals(1, topologyCount)
        val request = uploads.single()
        assertArrayEquals(preparedUpload.uploadId, request.uploadId)
        assertSame(preparedUpload.metadata, request.metadata)
        assertSame(preparedUpload.payload, request.payload)
        assertEquals(firstNode, request.node)
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.COMPLETED), queue.mutations)
    }

    @Test
    fun `second node success reuses the prepared values and completes`() {
        val queue = RecordingUploadQueue(queuedEntry)
        val requests = mutableListOf<UploadRequest>()
        val executor =
            executor(
                queue = queue,
                fetchTopology = { _, _, _, _ -> listOf(firstNode, secondNode) },
                upload = { request ->
                    requests += request
                    if (requests.size == FIRST_ATTEMPT) {
                        throw UploadException(UPLOAD_FAILED_MESSAGE)
                    }
                    preparedUpload.uploadId
                },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.COMPLETED, outcome)
        assertEquals(listOf(firstNode, secondNode), requests.map { it.node })
        for (request in requests) {
            assertArrayEquals(preparedUpload.uploadId, request.uploadId)
            assertSame(preparedUpload.metadata, request.metadata)
            assertSame(preparedUpload.payload, request.payload)
        }
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.COMPLETED), queue.mutations)
    }

    @Test
    fun `all node failures mark in flight then failed`() {
        val queue = RecordingUploadQueue(queuedEntry)
        val executor =
            executor(
                queue = queue,
                fetchTopology = { _, _, _, _ -> listOf(firstNode, secondNode) },
                upload = { throw UploadException(UPLOAD_FAILED_MESSAGE) },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.FAILED, outcome)
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.FAILED), queue.mutations)
    }

    @Test
    fun `empty topology returns no nodes and marks failed`() {
        val queue = RecordingUploadQueue(queuedEntry)
        var uploadCount = 0
        val executor =
            executor(
                queue = queue,
                fetchTopology = { _, _, _, _ -> emptyList() },
                upload = {
                    uploadCount += 1
                    preparedUpload.uploadId
                },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.NO_NODES, outcome)
        assertEquals(0, uploadCount)
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.FAILED), queue.mutations)
    }

    @Test
    fun `preparation failure marks failed without requesting the topology or uploading`() {
        val queue = RecordingUploadQueue(queuedEntry)
        var topologyCount = 0
        var uploadCount = 0
        val executor =
            executor(
                queue = queue,
                prepare = { _, _ -> throw UploadPreparationException(PREPARATION_FAILED_MESSAGE) },
                fetchTopology = { _, _, _, _ ->
                    topologyCount += 1
                    listOf(firstNode)
                },
                upload = {
                    uploadCount += 1
                    preparedUpload.uploadId
                },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.FAILED, outcome)
        assertEquals(0, topologyCount)
        assertEquals(0, uploadCount)
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.FAILED), queue.mutations)
    }

    @Test
    fun `topology failure marks failed without uploading`() {
        val queue = RecordingUploadQueue(queuedEntry)
        var uploadCount = 0
        val executor =
            executor(
                queue = queue,
                fetchTopology = { _, _, _, _ -> throw TopologyException(TOPOLOGY_FAILED_MESSAGE) },
                upload = {
                    uploadCount += 1
                    preparedUpload.uploadId
                },
            )

        val outcome = executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(PendingUploadOutcome.FAILED, outcome)
        assertEquals(0, uploadCount)
        assertEquals(listOf(ExecutedStep.IN_FLIGHT, ExecutedStep.FAILED), queue.mutations)
    }

    @Test
    fun `the entry is in flight before preparation`() {
        val steps = mutableListOf<ExecutedStep>()
        val queue = RecordingUploadQueue(queuedEntry, steps)
        val executor =
            executor(
                queue = queue,
                prepare = { _, _ ->
                    steps += ExecutedStep.PREPARE
                    preparedUpload
                },
                fetchTopology = { _, _, _, _ ->
                    steps += ExecutedStep.FETCH_TOPOLOGY
                    listOf(firstNode)
                },
                upload = {
                    steps += ExecutedStep.UPLOAD
                    preparedUpload.uploadId
                },
            )

        executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(
            listOf(
                ExecutedStep.IN_FLIGHT,
                ExecutedStep.PREPARE,
                ExecutedStep.FETCH_TOPOLOGY,
                ExecutedStep.UPLOAD,
                ExecutedStep.COMPLETED,
            ),
            steps,
        )
    }

    @Test
    fun `nodes are uploaded in topology order`() {
        val uploadedNodes = mutableListOf<TopologyNode>()
        val topologyOrder = listOf(thirdNode, firstNode, secondNode)
        val executor =
            executor(
                fetchTopology = { _, _, _, _ -> topologyOrder },
                upload = { request ->
                    uploadedNodes += request.node
                    throw UploadException(UPLOAD_FAILED_MESSAGE)
                },
            )

        executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(topologyOrder, uploadedNodes)
    }

    @Test
    fun `credentials are passed to the topology fetch and the upload requests`() {
        var fetchedHost: String? = null
        var fetchedPort: Int? = null
        var fetchedLogin: String? = null
        var fetchedPassword: String? = null
        val requests = mutableListOf<UploadRequest>()
        val executor =
            executor(
                fetchTopology = { host, port, login, password ->
                    fetchedHost = host
                    fetchedPort = port
                    fetchedLogin = login
                    fetchedPassword = password
                    listOf(firstNode)
                },
                upload = { request ->
                    requests += request
                    preparedUpload.uploadId
                },
            )

        executor.execute(queuedEntry, URI, SERVER_HOST, SERVER_PORT, LOGIN, PASSWORD)

        assertEquals(SERVER_HOST, fetchedHost)
        assertEquals(SERVER_PORT, fetchedPort)
        assertEquals(LOGIN, fetchedLogin)
        assertEquals(PASSWORD, fetchedPassword)
        val request = requests.single()
        assertEquals(LOGIN, request.login)
        assertEquals(PASSWORD, request.password)
    }

    private fun executor(
        queue: UploadQueue = RecordingUploadQueue(queuedEntry),
        prepare: (UploadQueueEntry, String) -> PreparedUpload = { _, _ -> preparedUpload },
        fetchTopology: (String, Int, String, String) -> List<TopologyNode> = { _, _, _, _ -> listOf(firstNode) },
        upload: (UploadRequest) -> ByteArray = { preparedUpload.uploadId },
    ): PendingUploadExecutor =
        PendingUploadExecutor(
            queue = queue,
            prepare = prepare,
            fetchTopology = fetchTopology,
            upload = upload,
        )

    private fun newEntry(): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = UPLOAD_ID,
            sourceId = SOURCE_ID,
            identity = MediaVersionIdentity(CONTENT_SHA256, ORIGINAL_NAME, TRACKED_TIME_MILLIS),
            sizeBytes = SIZE_BYTES,
            attemptCount = ATTEMPT_COUNT,
            state = UploadQueueState.PENDING,
        )

    private fun newPreparedUpload(): PreparedUpload =
        PreparedUpload(
            uploadId = PREPARED_UPLOAD_ID,
            metadata =
                UploadMetadata(
                    originalName = ORIGINAL_NAME,
                    timestampMillis = TRACKED_TIME_MILLIS,
                    originalSizeBytes = SIZE_BYTES,
                    compressedSizeBytes = PAYLOAD_BYTES.size.toLong(),
                    sha256 = SHA256.copyOf(),
                ),
            payload = CompressedPayload(listOf(PAYLOAD_BYTES)),
        )

    private fun node(
        label: String,
        host: String,
        port: Int,
    ): TopologyNode =
        TopologyNode(
            senderId = ByteArray(SENDER_ID_BYTES) { index -> index.toByte() },
            label = label,
            host = host,
            port = port,
            x25519PublicKey = ByteArray(KEY_BYTES) { index -> (index + 1).toByte() },
            ed25519PublicKey = ByteArray(KEY_BYTES) { index -> (index + 2).toByte() },
        )

    private class RecordingUploadQueue(
        private val entry: UploadQueueEntry,
        private val steps: MutableList<ExecutedStep> = mutableListOf(),
    ) : UploadQueue {
        val mutations: List<ExecutedStep>
            get() = steps.filter { step -> step in QUEUE_MUTATIONS }

        override fun enqueue(
            uploadId: String,
            sourceId: Long,
            identity: MediaVersionIdentity,
            sizeBytes: Long,
        ): UploadQueueEntry = entry

        override fun findVersion(identity: MediaVersionIdentity): UploadQueueEntry? = entry

        override fun restore(entries: List<UploadQueueEntry>) = Unit

        override fun entries(): List<UploadQueueEntry> = listOf(entry)

        override fun nextPending(): UploadQueueEntry? = entry

        override fun markInFlight(uploadId: String) {
            steps += ExecutedStep.IN_FLIGHT
        }

        override fun markCompleted(uploadId: String) {
            steps += ExecutedStep.COMPLETED
        }

        override fun markFailed(uploadId: String) {
            steps += ExecutedStep.FAILED
        }

        override fun requeueFailed(): Int = NO_REQUEUED_ENTRIES

        override fun pendingCount(): Int = PENDING_ENTRIES
    }

    private enum class ExecutedStep {
        IN_FLIGHT,
        PREPARE,
        FETCH_TOPOLOGY,
        UPLOAD,
        COMPLETED,
        FAILED,
    }

    private companion object {
        const val URI = "content://media/external/file/1"
        const val UPLOAD_ID = "123e4567-e89b-42d3-a456-426614174000"
        const val SOURCE_ID = 1L
        const val SIZE_BYTES = 4_096L
        const val ATTEMPT_COUNT = 0
        const val ORIGINAL_NAME = "holiday photo.jpg"
        const val TRACKED_TIME_MILLIS = 1_700_000_123_456L
        const val SHA256_HEX_CHARS = 64
        const val SHA256_BYTES = 32
        const val SENDER_ID_BYTES = 16
        const val KEY_BYTES = 32
        const val SERVER_HOST = "dedicated.example"
        const val SERVER_PORT = 8_443
        const val LOGIN = "test-login"
        const val PASSWORD = "test-password"
        const val FIRST_LABEL = "node-alpha"
        const val FIRST_HOST = "10.0.0.1"
        const val FIRST_PORT = 9_001
        const val SECOND_LABEL = "node-beta"
        const val SECOND_HOST = "10.0.0.2"
        const val SECOND_PORT = 9_002
        const val THIRD_LABEL = "node-gamma"
        const val THIRD_HOST = "10.0.0.3"
        const val THIRD_PORT = 9_003
        const val FIRST_ATTEMPT = 1
        const val NO_REQUEUED_ENTRIES = 0
        const val PENDING_ENTRIES = 1
        const val UPLOAD_FAILED_MESSAGE = "the upload request failed"
        const val PREPARATION_FAILED_MESSAGE = "the queued version could not be prepared"
        const val TOPOLOGY_FAILED_MESSAGE = "the topology request failed"

        val CONTENT_SHA256 = "a".repeat(SHA256_HEX_CHARS)
        val PREPARED_UPLOAD_ID = ByteArray(KEY_BYTES) { index -> index.toByte() }
        val SHA256 = ByteArray(SHA256_BYTES) { index -> (index + 1).toByte() }
        val PAYLOAD_BYTES = "compressed payload".toByteArray(Charsets.UTF_8)

        val QUEUE_MUTATIONS =
            setOf(ExecutedStep.IN_FLIGHT, ExecutedStep.COMPLETED, ExecutedStep.FAILED)
    }
}
