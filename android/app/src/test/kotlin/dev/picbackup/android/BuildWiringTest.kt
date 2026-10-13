package dev.picbackup.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BuildWiringTest {

    @Test
    fun applicationIdIsDevPicBackupAndroid() {
        assertEquals(EXPECTED_APPLICATION_ID, BuildConfig.APPLICATION_ID)
    }

    @Test
    fun debugUnitTestsReportDebugBuildType() {
        assertEquals(DEBUG_BUILD_TYPE, BuildConfig.BUILD_TYPE)
    }

    @Test
    fun debugUnitTestsHaveDebugFlagEnabled() {
        assertTrue(BuildConfig.DEBUG)
    }

    private companion object {
        const val EXPECTED_APPLICATION_ID = "dev.picbackup.android"
        const val DEBUG_BUILD_TYPE = "debug"
    }
}
