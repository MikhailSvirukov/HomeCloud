package dev.picbackup.android.queue

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class FileUploadQueueStoreTest {

    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun writeThenReadRoundTripsEveryFieldOfEveryEntry() {
        val store = FileUploadQueueStore(storeFile())
        val entries =
            listOf(
                entry(
                    uploadId = UPLOAD_ID_A,
                    sourceId = SOURCE_ID_A,
                    sizeBytes = SIZE_BYTES_A,
                    dateModifiedEpochSeconds = DATE_MODIFIED_A,
                    attemptCount = ATTEMPT_COUNT,
                    state = UploadQueueState.FAILED,
                ),
                entry(
                    uploadId = UPLOAD_ID_B,
                    sourceId = SOURCE_ID_B,
                    sizeBytes = SIZE_BYTES_B,
                    dateModifiedEpochSeconds = DATE_MODIFIED_B,
                    attemptCount = 0,
                    state = UploadQueueState.PENDING,
                ),
            )

        store.write(entries)

        assertEquals(entries, store.read())
    }

    @Test
    fun readReturnsEmptyListWhenStoreFileDoesNotExist() {
        val store = FileUploadQueueStore(storeFile())

        assertTrue(store.read().isEmpty())
    }

    @Test
    fun writeEmptyListThenReadReturnsEmptyList() {
        val store = FileUploadQueueStore(storeFile())

        store.write(emptyList())

        assertTrue(store.read().isEmpty())
    }

    @Test
    fun uploadIdWithTabNewlineAndBackslashRoundTripsExactly() {
        val store = FileUploadQueueStore(storeFile())
        val entry = entry(uploadId = ESCAPED_UPLOAD_ID)

        store.write(listOf(entry))

        assertEquals(listOf(entry), store.read())
    }

    @Test
    fun uploadIdWithCarriageReturnRoundTripsExactly() {
        val store = FileUploadQueueStore(storeFile())
        val entry = entry(uploadId = CARRIAGE_RETURN_UPLOAD_ID)

        store.write(listOf(entry))

        assertEquals(listOf(entry), store.read())
    }

    @Test
    fun readThrowsWhenUploadIdEscapingIsMalformed() {
        val trailingEscape = storeWithRawText(rawLine(uploadId = TRAILING_ESCAPE_UPLOAD_ID))
        val unknownEscape = storeWithRawText(rawLine(uploadId = UNKNOWN_ESCAPE_UPLOAD_ID))

        assertThrows(UploadQueueStoreException::class.java) { trailingEscape.read() }
        assertThrows(UploadQueueStoreException::class.java) { unknownEscape.read() }
    }

    @Test
    fun readThrowsWhenLineHasWrongFieldCount() {
        val file = storeFile()
        val store = FileUploadQueueStore(file)
        store.write(listOf(entry(uploadId = UPLOAD_ID_A)))
        file.appendText(NEWLINE + WRONG_FIELD_COUNT_LINE)

        val exception = assertThrows(UploadQueueStoreException::class.java) { store.read() }

        assertTrue(exception.message.orEmpty().contains(MALFORMED_LINE_NUMBER.toString()))
    }

    @Test
    fun readThrowsWhenNumericFieldIsNotANumber() {
        val store = storeWithRawText(rawLine(sizeBytes = NOT_A_NUMBER))

        assertThrows(UploadQueueStoreException::class.java) { store.read() }
    }

    @Test
    fun readThrowsWhenStateNameIsUnknown() {
        val store = storeWithRawText(rawLine(state = UNKNOWN_STATE))

        assertThrows(UploadQueueStoreException::class.java) { store.read() }
    }

    @Test
    fun writeReplacesPreviousSnapshotInsteadOfAppending() {
        val store = FileUploadQueueStore(storeFile())

        store.write(listOf(entry(uploadId = UPLOAD_ID_A)))
        store.write(listOf(entry(uploadId = UPLOAD_ID_B)))

        assertEquals(listOf(entry(uploadId = UPLOAD_ID_B)), store.read())
    }

    @Test
    fun writeLeavesNoTemporaryFileNextToStore() {
        val directory = temporaryFolder.newFolder()
        val store = FileUploadQueueStore(File(directory, STORE_FILE_NAME))

        store.write(listOf(entry(uploadId = UPLOAD_ID_A)))

        val remainingNames = directory.listFiles().orEmpty().map { stored -> stored.name }
        assertEquals(listOf(STORE_FILE_NAME), remainingNames)
    }

    private fun storeFile(): File = File(temporaryFolder.newFolder(), STORE_FILE_NAME)

    private fun storeWithRawText(text: String): FileUploadQueueStore {
        val file = storeFile()
        file.writeText(text, Charsets.UTF_8)
        return FileUploadQueueStore(file)
    }

    private fun rawLine(
        uploadId: String = UPLOAD_ID_A,
        sourceId: String = SOURCE_ID_A.toString(),
        sizeBytes: String = SIZE_BYTES_A.toString(),
        dateModifiedEpochSeconds: String = DATE_MODIFIED_A.toString(),
        attemptCount: String = ATTEMPT_COUNT.toString(),
        state: String = UploadQueueState.PENDING.name,
    ): String =
        listOf(uploadId, sourceId, sizeBytes, dateModifiedEpochSeconds, attemptCount, state)
            .joinToString(separator = FIELD_SEPARATOR)

    private fun entry(
        uploadId: String,
        sourceId: Long = SOURCE_ID_A,
        sizeBytes: Long = SIZE_BYTES_A,
        dateModifiedEpochSeconds: Long = DATE_MODIFIED_A,
        attemptCount: Int = ATTEMPT_COUNT,
        state: UploadQueueState = UploadQueueState.IN_FLIGHT,
    ): UploadQueueEntry =
        UploadQueueEntry(
            uploadId = uploadId,
            sourceId = sourceId,
            sizeBytes = sizeBytes,
            dateModifiedEpochSeconds = dateModifiedEpochSeconds,
            attemptCount = attemptCount,
            state = state,
        )

    private companion object {
        const val STORE_FILE_NAME = "upload-queue.txt"
        const val FIELD_SEPARATOR = "\t"
        const val NEWLINE = "\n"

        const val UPLOAD_ID_A = "upload-a"
        const val UPLOAD_ID_B = "upload-b"

        const val SOURCE_ID_A = 1L
        const val SOURCE_ID_B = 2L
        const val SIZE_BYTES_A = 1024L
        const val SIZE_BYTES_B = 2048L
        const val DATE_MODIFIED_A = 1_700_000_000L
        const val DATE_MODIFIED_B = 1_700_000_100L
        const val ATTEMPT_COUNT = 3

        const val ESCAPED_UPLOAD_ID = "tab\tnewline\nbackslash\\"
        const val CARRIAGE_RETURN_UPLOAD_ID = "carriage\rreturn"
        const val TRAILING_ESCAPE_UPLOAD_ID = "trailing\\"
        const val UNKNOWN_ESCAPE_UPLOAD_ID = "unknown\\x"
        const val WRONG_FIELD_COUNT_LINE = UPLOAD_ID_A
        const val MALFORMED_LINE_NUMBER = 2
        const val NOT_A_NUMBER = "not-a-number"
        const val UNKNOWN_STATE = "ARCHIVED"
    }
}
