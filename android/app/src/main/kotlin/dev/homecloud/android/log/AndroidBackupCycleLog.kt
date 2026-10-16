package dev.homecloud.android.log

import android.util.Log
import dev.homecloud.android.backup.PlanResult

/** [BackupCycleLog] that reports each cycle as one line in the Android log. */
class AndroidBackupCycleLog : BackupCycleLog {

    override fun cyclePlanned(result: PlanResult) {
        Log.i(
            TAG,
            "$PLANNED_MESSAGE $DISCOVERED_FIELD=${result.discoveredCandidates} " +
                "$NEWLY_ENQUEUED_FIELD=${result.newlyEnqueued}",
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
