package dev.homecloud.android.log

import dev.homecloud.android.backup.PlanResult

/**
 * Structured record of what a backup cycle did, carrying only cycle counts by construction, so the
 * area rule that file names, payloads and credentials must never be logged cannot be broken here.
 */
interface BackupCycleLog {

    /** Records a cycle that completed. */
    fun cyclePlanned(result: PlanResult)

    /** Records a cycle that did not complete. */
    fun cycleFailed()
}

/** [BackupCycleLog] that records nothing and neither allocates nor performs work. */
object NoOpBackupCycleLog : BackupCycleLog {

    override fun cyclePlanned(result: PlanResult) = Unit

    override fun cycleFailed() = Unit
}
