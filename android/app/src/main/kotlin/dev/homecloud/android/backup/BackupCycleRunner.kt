package dev.homecloud.android.backup

import dev.homecloud.android.log.BackupCycleLog
import dev.homecloud.android.log.NoOpBackupCycleLog
import dev.homecloud.android.media.MediaLibraryUnreadableException
import dev.homecloud.android.queue.UploadQueueStore
import dev.homecloud.android.queue.UploadQueueStoreException
import dev.homecloud.android.status.CycleOutcome
import dev.homecloud.android.status.LastCycleRecord
import dev.homecloud.android.status.LastCycleStore
import dev.homecloud.android.status.LastErrorCategory
import java.io.IOException
import java.util.logging.Level
import java.util.logging.Logger

/**
 * Sequences one backup cycle over the media planner and the queue store.
 *
 * The cycle is synchronous and performs blocking store I/O; threading and backgrounding belong to
 * the caller.
 *
 * Each completed cycle reports how many candidates were discovered, newly enqueued, and refused
 * because the queue had reached its configured capacity (`CFG-AND-002 / 2`). Already accepted
 * entries are persisted untouched when the queue is full (`NFR-REL-001 / 1`). The result of the
 * cycle is recorded for the main screen when a last-cycle store is supplied: a completed cycle is
 * recorded as a success and a failed cycle as a failure with its error category
 * (`FR-AND-044 / 1`, `FR-AND-045 / 1`, `FR-AND-047 / 1`). Recording never changes the cycle
 * outcome, and the recorded category never carries a file name or payload.
 */
internal class BackupCycleRunner(
    private val planner: MediaBackupPlanner,
    private val store: UploadQueueStore,
    private val log: BackupCycleLog = NoOpBackupCycleLog,
    private val lastCycleStore: LastCycleStore? = null,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    /**
     * Restores the stored snapshot before planning, so work that is already queued is known and not
     * planned again, then plans newly discovered media into the queue, then persists it, so the
     * newly queued work survives a restart. Reports a completed cycle through the log, including
     * any candidates rejected by capacity, or reports a failure and rethrows it unchanged.
     */
    fun runOnce(): PlanResult {
        val planResult =
            try {
                planner.queue.restore(store.read())
                val planned = planner.plan()
                store.write(planner.queue.entries())
                planned
            } catch (failure: Exception) {
                log.cycleFailed()
                record(LastCycleRecord(clock(), CycleOutcome.FAILURE, categoryOf(failure)))
                throw failure
            }
        record(LastCycleRecord(clock(), CycleOutcome.SUCCESS, null))
        log.cyclePlanned(
            planResult.discoveredCandidates,
            planResult.newlyEnqueued,
            planResult.rejectedByCapacity,
        )
        return planResult
    }

    private fun record(lastCycle: LastCycleRecord) {
        val target = lastCycleStore ?: return
        try {
            target.write(lastCycle)
        } catch (_: Exception) {
            LOGGER.log(Level.WARNING, RECORD_FAILED_MESSAGE)
        }
    }

    private companion object {
        val LOGGER: Logger = Logger.getLogger(BackupCycleRunner::class.java.name)

        const val RECORD_FAILED_MESSAGE = "Could not record the last backup cycle result"
    }
}

/**
 * Classifies a cycle failure for the main screen (`FR-AND-047 / 1`): an unreadable media library,
 * an unavailable queue, or any other failure. The category is derived from the exception type and
 * never from its message.
 */
private fun categoryOf(failure: Exception): LastErrorCategory =
    when (failure) {
        is MediaLibraryUnreadableException -> LastErrorCategory.MEDIA_LIBRARY_UNREADABLE
        is UploadQueueStoreException -> LastErrorCategory.QUEUE_UNAVAILABLE
        is IOException -> LastErrorCategory.QUEUE_UNAVAILABLE
        else -> LastErrorCategory.INTERNAL
    }
