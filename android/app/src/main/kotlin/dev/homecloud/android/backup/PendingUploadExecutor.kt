package dev.homecloud.android.backup

import dev.homecloud.android.protocol.TopologyNode
import dev.homecloud.android.queue.UploadQueue
import dev.homecloud.android.queue.UploadQueueEntry
import dev.homecloud.android.upload.UploadRequest
import kotlin.coroutines.cancellation.CancellationException

/** Outcome of one pending-entry execution. */
internal enum class PendingUploadOutcome {
    COMPLETED,
    FAILED,
    NO_NODES,
}

/**
 * Executes one pending queue entry against the storage nodes of the topology.
 *
 * The entry is marked in flight before anything else happens and prepared exactly once through the
 * supplied preparation, which turns the entry and its source URI into the upload arguments and
 * payload. The node list is requested once from the dedicated server, and its nodes are tried in
 * answer order with the same prepared values, because the upload operation restarts the version
 * from frame zero for every attempt (`FR-AND-017`, `FR-AND-030`). The first node that confirms the
 * upload completes the entry (`FR-UP-021`); a preparation failure, a topology failure, an empty
 * topology or a failed upload on every node marks it failed, so an unfinished upload stays queued
 * (`FR-AND-027`).
 *
 * The queue records the in-flight, completed and failed states, the preparation and topology
 * request run once per execution, and the upload runs once per tried node. A failure of the queue
 * itself is propagated to the caller instead of being treated as an upload failure. The executor
 * holds no state between calls, produces no log output, and never places credentials, names,
 * payload bytes or digests into an outcome; a node failure's cause is not carried because the
 * outcome is a summary and the upload operation reports its own failures.
 */
internal class PendingUploadExecutor(
    private val queue: UploadQueue,
    private val prepare: (entry: UploadQueueEntry, uri: String) -> PreparedUpload,
    private val fetchTopology: (host: String, port: Int, login: String, password: String) -> List<TopologyNode>,
    private val upload: (UploadRequest) -> ByteArray,
) {
    /**
     * Prepares [entry] once, requests the topology once and uploads to the nodes in answer order
     * until one confirms, then records the queue state:
     * in flight before the attempt, completed after a confirmation, failed when no node confirms.
     */
    fun execute(
        entry: UploadQueueEntry,
        uri: String,
        serverHost: String,
        serverPort: Int,
        login: String,
        password: String,
    ): PendingUploadOutcome {
        queue.markInFlight(entry.uploadId)
        val prepared =
            try {
                prepare(entry, uri)
            } catch (exception: CancellationException) {
                throw exception
            } catch (_: Exception) {
                queue.markFailed(entry.uploadId)
                return PendingUploadOutcome.FAILED
            }
        val nodes =
            try {
                fetchTopology(serverHost, serverPort, login, password)
            } catch (exception: CancellationException) {
                throw exception
            } catch (_: Exception) {
                queue.markFailed(entry.uploadId)
                return PendingUploadOutcome.FAILED
            }
        if (nodes.isEmpty()) {
            queue.markFailed(entry.uploadId)
            return PendingUploadOutcome.NO_NODES
        }
        for (node in nodes) {
            val confirmed =
                try {
                    upload(
                        UploadRequest(
                            uploadId = prepared.uploadId,
                            node = node,
                            login = login,
                            password = password,
                            metadata = prepared.metadata,
                            payload = prepared.payload,
                        ),
                    )
                    true
                } catch (exception: CancellationException) {
                    throw exception
                } catch (_: Exception) {
                    false
                }
            if (confirmed) {
                queue.markCompleted(entry.uploadId)
                return PendingUploadOutcome.COMPLETED
            }
        }
        queue.markFailed(entry.uploadId)
        return PendingUploadOutcome.FAILED
    }
}
