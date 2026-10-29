package dev.homecloud.android.status

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

class FileLastCycleStoreTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun `write then read round-trips a successful record with an empty error`() {
        val file = storeFile()
        val store = FileLastCycleStore(file)
        val record = LastCycleRecord(COMPLETED_AT_MILLIS, CycleOutcome.SUCCESS, null)

        store.write(record)

        assertEquals(record, store.read())
        assertTrue(file.readText(Charsets.UTF_8).endsWith("$ERROR_FIELD=\n"))
    }

    @Test
    fun `write then read round-trips a failed record with its category`() {
        val store = FileLastCycleStore(storeFile())
        val record =
            LastCycleRecord(
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.FAILURE,
                error = LastErrorCategory.TRANSPORT_UNAVAILABLE,
            )

        store.write(record)

        assertEquals(record, store.read())
    }

    @Test
    fun `read returns null when the store file does not exist`() {
        val store = FileLastCycleStore(storeFile())

        assertNull(store.read())
    }

    @Test
    fun `read returns null when a required line is missing`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$COMPLETED_AT_MILLIS",
                    "$OUTCOME_FIELD=SUCCESS",
                ).joinToString(separator = NEWLINE),
            )

        assertNull(store.read())
    }

    @Test
    fun `read returns null when a field key is unknown`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$COMPLETED_AT_MILLIS",
                    "$OUTCOME_FIELD=SUCCESS",
                    "result=",
                ).joinToString(separator = NEWLINE),
            )

        assertNull(store.read())
    }

    @Test
    fun `read returns null when the completion time is not numeric`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$NOT_A_NUMBER",
                    "$OUTCOME_FIELD=SUCCESS",
                    "$ERROR_FIELD=",
                ).joinToString(separator = NEWLINE),
            )

        assertNull(store.read())
    }

    @Test
    fun `read returns null when the outcome is unknown`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$COMPLETED_AT_MILLIS",
                    "$OUTCOME_FIELD=$UNKNOWN_OUTCOME",
                    "$ERROR_FIELD=",
                ).joinToString(separator = NEWLINE),
            )

        assertNull(store.read())
    }

    @Test
    fun `read returns null when a successful record carries an error`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$COMPLETED_AT_MILLIS",
                    "$OUTCOME_FIELD=SUCCESS",
                    "$ERROR_FIELD=${LastErrorCategory.INTERNAL.name}",
                ).joinToString(separator = NEWLINE),
            )

        assertNull(store.read())
    }

    @Test
    fun `read accepts one trailing newline`() {
        val store =
            storeWithRawText(
                listOf(
                    "$COMPLETED_AT_FIELD=$COMPLETED_AT_MILLIS",
                    "$OUTCOME_FIELD=FAILURE",
                    "$ERROR_FIELD=${LastErrorCategory.MEDIA_LIBRARY_UNREADABLE.name}",
                ).joinToString(separator = NEWLINE) + NEWLINE,
            )

        assertEquals(
            LastCycleRecord(
                completedAtMillis = COMPLETED_AT_MILLIS,
                outcome = CycleOutcome.FAILURE,
                error = LastErrorCategory.MEDIA_LIBRARY_UNREADABLE,
            ),
            store.read(),
        )
    }

    @Test
    fun `write over an existing file replaces the previous record`() {
        val store = FileLastCycleStore(storeFile())
        val first = LastCycleRecord(COMPLETED_AT_MILLIS, CycleOutcome.FAILURE, LastErrorCategory.INTERNAL)
        val second = LastCycleRecord(COMPLETED_AT_MILLIS + 1, CycleOutcome.SUCCESS, null)

        store.write(first)
        store.write(second)

        assertEquals(second, store.read())
    }

    private fun storeFile(): File = File(temporaryFolder.newFolder(), STORE_FILE_NAME)

    private fun storeWithRawText(text: String): FileLastCycleStore {
        val file = storeFile()
        file.writeText(text, Charsets.UTF_8)
        return FileLastCycleStore(file)
    }

    private companion object {
        const val STORE_FILE_NAME = "last-cycle"
        const val COMPLETED_AT_MILLIS = 1_700_000_000_000L

        const val COMPLETED_AT_FIELD = "completedAt"
        const val OUTCOME_FIELD = "outcome"
        const val ERROR_FIELD = "error"

        const val NEWLINE = "\n"
        const val NOT_A_NUMBER = "not-a-number"
        const val UNKNOWN_OUTCOME = "PARTIAL"
    }
}
