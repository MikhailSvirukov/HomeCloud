package dev.homecloud.android.log

import android.util.Log

/**
 * [BackupCycleLog] that reports each cycle as one line in the Android log.
 *
 * The planned line carries the discovered, newly enqueued, and capacity-rejected counts, so
 * reaching the configured queue maximum is reported without logging any file name or payload
 * (`CFG-AND-002 / 2`).
 */
internal class AndroidBackupCycleLog : BackupCycleLog {
    override fun cyclePlanned(
        discoveredCandidates: Int,
        newlyEnqueued: Int,
        rejectedByCapacity: Int,
    ) {
        Log.i(
            TAG,
            "$PLANNED_MESSAGE $DISCOVERED_FIELD=$discoveredCandidates " +
                "$NEWLY_ENQUEUED_FIELD=$newlyEnqueued " +
                "$REJECTED_BY_CAPACITY_FIELD=$rejectedByCapacity",
        )
    }

    override fun cycleFailed() {
        Log.w(TAG, FAILED_MESSAGE)
    }

    private companion object {
        const val TAG = "HomeCloudBackupCycle"
        const val PLANNED_MESSAGE = "Backup cycle planned"
        const val DISCOVERED_FIELD = "discovered"
        const val NEWLY_ENQUEUED_FIELD = "newlyEnqueued"
        const val REJECTED_BY_CAPACITY_FIELD = "rejectedByCapacity"
        const val FAILED_MESSAGE = "Backup cycle failed"
    }
}
