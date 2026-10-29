package dev.homecloud.android

import android.app.Application
import androidx.work.Configuration
import dev.homecloud.android.backup.BackupCycleComposition
import dev.homecloud.android.backup.BackupWorkerFactory

/**
 * Application entry point that supplies WorkManager's on-demand configuration (`CON-AND-001 / 1`).
 *
 * The configuration installs the backup worker factory so every scheduled cycle is built with the
 * real media, queue, and persistence graph. Initialization stays on demand: the manifest removes
 * the default startup initializer, so WorkManager is created from this configuration on first use
 * and a test harness can install its own WorkManager before any work runs.
 *
 * No work is enqueued here: scheduling belongs to the screen that starts a cycle, and enqueuing
 * during application startup would run before a test harness has installed its scheduler.
 */
internal class HomeCloudApplication :
    Application(),
    Configuration.Provider {
    override val workManagerConfiguration: Configuration
        get() =
            Configuration
                .Builder()
                .setWorkerFactory(BackupWorkerFactory { BackupCycleComposition.runner(this) })
                .build()
}
