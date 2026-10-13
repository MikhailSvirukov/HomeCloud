package dev.picbackup.android.backup

import dev.picbackup.android.queue.UploadQueue
import dev.picbackup.android.queue.UploadQueueStore

/**
 * Sequences one backup cycle across planning and the local upload queue: it restores the persisted
 * queue, plans newly discovered media into it, and persists the result, so a cycle can continue
 * where the previous one stopped.
 *
 * The cycle is synchronous and performs blocking store I/O; threading and backgrounding belong to
 * the caller.
 */
class BackupCycleRunner(
    private val planner: MediaBackupPlanner,
    private val queue: UploadQueue,
    private val store: UploadQueueStore,
) {

    /**
     * Restores the stored snapshot before planning, so work that is already queued is known and not
     * planned again, then plans newly discovered media into the queue, then persists it, so the
     * newly queued work survives a restart.
     */
    fun runOnce(): PlanResult {
        queue.restore(store.read())
        val planResult = planner.plan()
        store.write(queue.entries())
        return planResult
    }
}
