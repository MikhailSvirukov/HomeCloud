package dev.homecloud.android.backup

import dev.homecloud.android.log.BackupCycleLog
import dev.homecloud.android.log.NoOpBackupCycleLog
import dev.homecloud.android.queue.UploadQueueStore

/**
 * Sequences one backup cycle over the media planner and the queue store.
 *
 * The cycle is synchronous and performs blocking store I/O; threading and backgrounding belong to
 * the caller.
 */
class BackupCycleRunner(
    private val planner: MediaBackupPlanner,
    private val store: UploadQueueStore,
    private val log: BackupCycleLog = NoOpBackupCycleLog,
) {

    /**
     * Restores the stored snapshot before planning, so work that is already queued is known and not
     * planned again, then plans newly discovered media into the queue, then persists it, so the
     * newly queued work survives a restart. Reports a completed cycle through the log, or reports a
     * failure and rethrows it unchanged.
     */
    fun runOnce(): PlanResult {
        val planResult =
            try {
                planner.queue.restore(store.read())
                val planned = planner.plan()
                store.write(planner.queue.entries())
                planned
            } catch (failure: Throwable) {
                log.cycleFailed()
                throw failure
            }
        log.cyclePlanned(planResult)
        return planResult
    }
}
