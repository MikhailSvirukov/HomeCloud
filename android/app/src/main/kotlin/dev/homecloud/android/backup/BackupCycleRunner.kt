package dev.homecloud.android.backup

import dev.homecloud.android.log.BackupCycleLog
import dev.homecloud.android.log.NoOpBackupCycleLog
import dev.homecloud.android.queue.UploadQueueStore

/**
 * Sequences one backup cycle over the media planner and the queue store.
 *
 * The cycle is synchronous and performs blocking store I/O; threading and backgrounding belong to
 * the caller.
 *
 * Each completed cycle reports how many candidates were discovered, newly enqueued, and refused
 * because the queue had reached its configured capacity (`CFG-AND-002 / 2`). Already accepted
 * entries are persisted untouched when the queue is full (`NFR-REL-001 / 1`).
 */
internal class BackupCycleRunner(
    private val planner: MediaBackupPlanner,
    private val store: UploadQueueStore,
    private val log: BackupCycleLog = NoOpBackupCycleLog,
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
                throw failure
            }
        log.cyclePlanned(
            planResult.discoveredCandidates,
            planResult.newlyEnqueued,
            planResult.rejectedByCapacity,
        )
        return planResult
    }
}
