package dev.homecloud.android.log

/**
 * Structured record of what a backup cycle did, carrying only cycle counts by construction, so the
 * area rule that file names, payloads and credentials must never be logged cannot be broken here.
 *
 * Implementations must not throw: a log failure must not turn a completed cycle into a failure.
 */
internal interface BackupCycleLog {

    /** Records a cycle that completed. */
    fun cyclePlanned(discoveredCandidates: Int, newlyEnqueued: Int)

    /** Records a cycle that did not complete. */
    fun cycleFailed()
}

/** [BackupCycleLog] that records nothing and neither allocates nor performs work. */
internal object NoOpBackupCycleLog : BackupCycleLog {

    override fun cyclePlanned(discoveredCandidates: Int, newlyEnqueued: Int) = Unit

    override fun cycleFailed() = Unit
}
