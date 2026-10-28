package dev.homecloud.android

import androidx.test.core.app.ApplicationProvider
import androidx.work.testing.TestListenableWorkerBuilder
import dev.homecloud.android.backup.BackupWorker
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

/**
 * Verifies the application the merged manifest installs and the WorkManager configuration it
 * provides, so scheduled work is built through the backup worker factory.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [HomeCloudApplicationTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class HomeCloudApplicationTest {
    @Test
    fun `the merged manifest installs HomeCloudApplication`() {
        val application = ApplicationProvider.getApplicationContext<HomeCloudApplication>()

        assertEquals(HomeCloudApplication::class.java.name, application.applicationInfo.className)
    }

    @Test
    fun `the application configuration builds a backup worker`() {
        val application = ApplicationProvider.getApplicationContext<HomeCloudApplication>()

        val worker =
            TestListenableWorkerBuilder<BackupWorker>(application)
                .setWorkerFactory(application.workManagerConfiguration.workerFactory)
                .build()

        assertEquals(BackupWorker::class.java, worker.javaClass)
    }

    private companion object {
        const val SDK_INT = 33
    }
}
