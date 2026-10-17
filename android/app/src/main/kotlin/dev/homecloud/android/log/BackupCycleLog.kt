package dev.homecloud.android.log

/**
 * Structured record of what a backup cycle did, carrying only cycle counts by construction, so the
 * area rule that file names, payloads and credentials must never be logged cannot be broken here.
 *
 * A completed cycle reports discovered, newly enqueued, and capacity-rejected version counts, so a
 * queue that reached its configured maximum is visible (`CFG-AND-002 / 2`) while accepted entries
 * remain in place (`NFR-REL-001 / 1`).
 *
 * Implementations must not throw: a log failure must not turn a completed cycle into a failure.
 */
internal interface BackupCycleLog {
    /** Records a cycle that completed, including how many candidates its full queue refused. */
    fun cyclePlanned(
        discoveredCandidates: Int,
        newlyEnqueued: Int,
        rejectedByCapacity: Int,
    )

    /** Records a cycle that did not complete. */
    fun cycleFailed()
}

/** [BackupCycleLog] that records nothing and neither allocates nor performs work. */
internal object NoOpBackupCycleLog : BackupCycleLog {
    override fun cyclePlanned(
        discoveredCandidates: Int,
        newlyEnqueued: Int,
        rejectedByCapacity: Int,
    ) = Unit

    override fun cycleFailed() = Unit
}
