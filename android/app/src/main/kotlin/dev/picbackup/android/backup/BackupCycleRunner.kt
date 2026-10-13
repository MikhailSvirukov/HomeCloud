package dev.picbackup.android.backup

import dev.picbackup.android.queue.UploadQueueStore

/**
 * Sequences one backup cycle over the media planner and the queue store.
 *
 * The cycle is synchronous and performs blocking store I/O; threading and backgrounding belong to
 * the caller.
 */
class BackupCycleRunner(
    private val planner: MediaBackupPlanner,
    private val store: UploadQueueStore,
) {

    /**
     * Restores the stored snapshot before planning, so work that is already queued is known and not
     * planned again, then plans newly discovered media into the queue, then persists it, so the
     * newly queued work survives a restart.
     */
    fun runOnce(): PlanResult {
        planner.queue.restore(store.read())
        val planResult = planner.plan()
        store.write(planner.queue.entries())
        return planResult
    }
}
