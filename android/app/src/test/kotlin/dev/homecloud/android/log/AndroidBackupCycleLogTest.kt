package dev.homecloud.android.log

import android.util.Log
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import org.robolectric.shadows.ShadowLog

/**
 * Robolectric needs a concrete SDK; 30 matches the other Robolectric tests in this area and the
 * logged behaviour does not depend on the level.
 */
private const val SDK = 30

/**
 * Pins the `android.util.Log` lines [AndroidBackupCycleLog] emits for each cycle event.
 *
 * The emitted entries are read through Robolectric's [ShadowLog]; no production change relies on a
 * mocked `Log`, so the real tag/priority/message contract is exercised.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [SDK])
@ConscryptMode(ConscryptMode.Mode.OFF)
class AndroidBackupCycleLogTest {
    @Test
    fun `cyclePlanned writes one info line carrying only the two counts`() {
        ShadowLog.clear()

        AndroidBackupCycleLog().cyclePlanned(DISCOVERED_CANDIDATES, NEWLY_ENQUEUED)

        val logs = ShadowLog.getLogs()
        assertEquals(1, logs.size)
        val entry = logs.single()
        assertEquals(Log.INFO, entry.type)
        assertEquals(CYCLE_TAG, entry.tag)
        assertEquals(
            "Backup cycle planned discovered=$DISCOVERED_CANDIDATES newlyEnqueued=$NEWLY_ENQUEUED",
            entry.msg,
        )
    }

    @Test
    fun `cycleFailed writes one warn line carrying no counts`() {
        ShadowLog.clear()

        AndroidBackupCycleLog().cycleFailed()

        val logs = ShadowLog.getLogs()
        assertEquals(1, logs.size)
        val entry = logs.single()
        assertEquals(Log.WARN, entry.type)
        assertEquals(CYCLE_TAG, entry.tag)
        assertEquals("Backup cycle failed", entry.msg)
    }

    private companion object {
        const val DISCOVERED_CANDIDATES = 5
        const val NEWLY_ENQUEUED = 2

        /**
         * The tag is part of the log contract this test fixes, so it is pinned as a literal: a
         * change to the production tag must fail the test instead of moving both sides together.
         */
        const val CYCLE_TAG = "HomeCloudBackupCycle"
    }
}
