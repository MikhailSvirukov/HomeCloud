package dev.picbackup.android.queue

import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/**
 * [UploadQueueStore] that keeps the whole snapshot in one UTF-8 text file.
 *
 * The file holds one entry per line, separated by `\n`, with tab-separated fields in the order
 * `uploadId`, `sourceId`, `sizeBytes`, `dateModifiedEpochSeconds`, `attemptCount`, `state`. Inside
 * `uploadId` the backslash, tab, newline and carriage-return characters are escaped as `\\`, `\t`,
 * `\n` and `\r`, and the reader reverses that escaping. A write replaces the complete file through a
 * temporary sibling and a rename, so a partially written snapshot can never be read back; the
 * caller owns the directory and a missing directory is not created. Reading an absent or empty file
 * yields no entries, while a malformed line fails the read instead of dropping queued work silently.
 * Not thread-safe; access must stay on one thread or be externally synchronized.
 */
class FileUploadQueueStore(private val file: File) : UploadQueueStore {

    override fun read(): List<UploadQueueEntry> {
        if (!file.exists()) {
            return emptyList()
        }
        val content =
            try {
                file.readText(Charsets.UTF_8)
            } catch (cause: IOException) {
                throw UploadQueueStoreException("Could not read the upload queue store", cause)
            }
        if (content.isEmpty()) {
            return emptyList()
        }
        return content
            .split(NEWLINE)
            .mapIndexed { index, line -> parseLine(line, index + 1) }
    }

    override fun write(entries: List<UploadQueueEntry>) {
        val target = file.absoluteFile
        val directory =
            target.parentFile
                ?: throw UploadQueueStoreException("Could not resolve the upload queue store directory")
        val temporary =
            try {
                File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
            } catch (cause: IOException) {
                throw UploadQueueStoreException("Could not create a temporary upload queue store", cause)
            }
        try {
            temporary.writeText(formatSnapshot(entries), Charsets.UTF_8)
            moveOver(temporary, target)
        } catch (cause: IOException) {
            throw UploadQueueStoreException("Could not write the upload queue store", cause)
        } finally {
            if (temporary.exists()) {
                temporary.delete()
            }
        }
    }

    private fun formatSnapshot(entries: List<UploadQueueEntry>): String =
        entries.joinToString(separator = NEWLINE) { entry -> formatLine(entry) }

    private fun formatLine(entry: UploadQueueEntry): String =
        listOf(
            escape(entry.uploadId),
            entry.sourceId.toString(),
            entry.sizeBytes.toString(),
            entry.dateModifiedEpochSeconds.toString(),
            entry.attemptCount.toString(),
            entry.state.name,
        ).joinToString(separator = FIELD_SEPARATOR)

    private fun parseLine(line: String, lineNumber: Int): UploadQueueEntry {
        val fields = line.split(FIELD_SEPARATOR)
        if (fields.size != FIELD_COUNT) {
            throw UploadQueueStoreException(
                "Malformed upload queue entry on line $lineNumber: expected $FIELD_COUNT fields " +
                    "but found ${fields.size}",
            )
        }
        return UploadQueueEntry(
            uploadId = unescape(fields[UPLOAD_ID_INDEX], lineNumber),
            sourceId = parseLong(fields[SOURCE_ID_INDEX], SOURCE_ID_FIELD, lineNumber),
            sizeBytes = parseLong(fields[SIZE_BYTES_INDEX], SIZE_BYTES_FIELD, lineNumber),
            dateModifiedEpochSeconds =
                parseLong(fields[DATE_MODIFIED_INDEX], DATE_MODIFIED_FIELD, lineNumber),
            attemptCount = parseInt(fields[ATTEMPT_COUNT_INDEX], ATTEMPT_COUNT_FIELD, lineNumber),
            state = parseState(fields[STATE_INDEX], lineNumber),
        )
    }

    private fun parseLong(field: String, fieldName: String, lineNumber: Int): Long =
        field.toLongOrNull()
            ?: throw UploadQueueStoreException(
                "Malformed upload queue entry on line $lineNumber: $fieldName is not a number",
            )

    private fun parseInt(field: String, fieldName: String, lineNumber: Int): Int =
        field.toIntOrNull()
            ?: throw UploadQueueStoreException(
                "Malformed upload queue entry on line $lineNumber: $fieldName is not a number",
            )

    private fun parseState(field: String, lineNumber: Int): UploadQueueState =
        UploadQueueState.entries.firstOrNull { state -> state.name == field }
            ?: throw UploadQueueStoreException(
                "Malformed upload queue entry on line $lineNumber: unknown state \"$field\"",
            )

    private fun escape(value: String): String =
        buildString(value.length) {
            value.forEach { char ->
                when (char) {
                    ESCAPE -> append(ESCAPE).append(ESCAPE)
                    TAB -> append(ESCAPE).append('t')
                    NEWLINE_CHAR -> append(ESCAPE).append('n')
                    CARRIAGE_RETURN -> append(ESCAPE).append('r')
                    else -> append(char)
                }
            }
        }

    private fun unescape(field: String, lineNumber: Int): String {
        val result = StringBuilder(field.length)
        var index = 0
        while (index < field.length) {
            val char = field[index]
            if (char != ESCAPE) {
                result.append(char)
                index += 1
                continue
            }
            if (index + 1 >= field.length) {
                throw UploadQueueStoreException(
                    "Malformed upload queue entry on line $lineNumber: trailing escape character",
                )
            }
            when (field[index + 1]) {
                ESCAPE -> result.append(ESCAPE)
                't' -> result.append(TAB)
                'n' -> result.append(NEWLINE_CHAR)
                'r' -> result.append(CARRIAGE_RETURN)
                else ->
                    throw UploadQueueStoreException(
                        "Malformed upload queue entry on line $lineNumber: unknown escape sequence",
                    )
            }
            index += 2
        }
        return result.toString()
    }

    private fun moveOver(source: File, target: File) {
        try {
            Files.move(
                source.toPath(),
                target.toPath(),
                StandardCopyOption.REPLACE_EXISTING,
                StandardCopyOption.ATOMIC_MOVE,
            )
        } catch (unsupported: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private companion object {
        const val FIELD_SEPARATOR = "\t"
        const val NEWLINE = "\n"
        const val FIELD_COUNT = 6

        const val UPLOAD_ID_INDEX = 0
        const val SOURCE_ID_INDEX = 1
        const val SIZE_BYTES_INDEX = 2
        const val DATE_MODIFIED_INDEX = 3
        const val ATTEMPT_COUNT_INDEX = 4
        const val STATE_INDEX = 5

        const val SOURCE_ID_FIELD = "sourceId"
        const val SIZE_BYTES_FIELD = "sizeBytes"
        const val DATE_MODIFIED_FIELD = "dateModifiedEpochSeconds"
        const val ATTEMPT_COUNT_FIELD = "attemptCount"

        const val ESCAPE = '\\'
        const val TAB = '\t'
        const val NEWLINE_CHAR = '\n'
        const val CARRIAGE_RETURN = '\r'

        const val TEMPORARY_FILE_PREFIX = "upload-queue-"
        const val TEMPORARY_FILE_SUFFIX = ".tmp"
    }
}

/** Signals that the persisted upload queue could not be read or written. */
class UploadQueueStoreException(message: String, cause: Throwable? = null) : Exception(message, cause)
