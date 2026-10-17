package dev.homecloud.android.log

import android.util.Log

/** [BackupCycleLog] that reports each cycle as one line in the Android log. */
internal class AndroidBackupCycleLog : BackupCycleLog {
    override fun cyclePlanned(
        discoveredCandidates: Int,
        newlyEnqueued: Int,
    ) {
        Log.i(
            TAG,
            "$PLANNED_MESSAGE $DISCOVERED_FIELD=$discoveredCandidates " +
                "$NEWLY_ENQUEUED_FIELD=$newlyEnqueued",
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
        const val FAILED_MESSAGE = "Backup cycle failed"
    }
}
