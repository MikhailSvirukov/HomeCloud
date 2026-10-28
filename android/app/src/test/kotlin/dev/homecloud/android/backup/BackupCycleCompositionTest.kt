package dev.homecloud.android.backup

import android.content.Context
import android.provider.MediaStore
import androidx.test.core.app.ApplicationProvider
import dev.homecloud.android.testsupport.RecordingMediaContentProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.io.File

/**
 * Runs one cycle through the real [BackupCycleComposition] graph against a media store with no
 * rows, so the production wiring of the scanner, queue, and on-disk store is exercised end to end.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [BackupCycleCompositionTest.SDK_INT])
@ConscryptMode(ConscryptMode.Mode.OFF)
class BackupCycleCompositionTest {
    @Test
    fun `runOnce over an empty media store plans nothing and persists the queue snapshot`() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        installEmptyMediaProvider()

        val result = BackupCycleComposition.runner(context).runOnce()

        assertEquals(PlanResult(discoveredCandidates = 0, newlyEnqueued = 0, rejectedByCapacity = 0), result)
        assertTrue(
            "expected the cycle to create the queue snapshot",
            File(context.filesDir, SNAPSHOT_FILE_NAME).exists(),
        )
    }

    private fun installEmptyMediaProvider() {
        val authority =
            requireNotNull(MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL).authority) {
                "media collection uri must carry an authority"
            }
        val provider =
            Robolectric
                .buildContentProvider(RecordingMediaContentProvider::class.java)
                .create(authority)
                .get()
        provider.rows = emptyList()
    }

    private companion object {
        const val SDK_INT = 30

        /** Mirrors the snapshot name the production composition keeps private. */
        const val SNAPSHOT_FILE_NAME = "upload-queue.snapshot"
    }
}
