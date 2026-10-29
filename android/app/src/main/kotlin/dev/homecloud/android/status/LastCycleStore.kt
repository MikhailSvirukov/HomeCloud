package dev.homecloud.android.status

import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/** Last completed backup cycle as the main screen reports it (`FR-AND-045 / 1`). */
internal data class LastCycleRecord(
    val completedAtMillis: Long,
    val outcome: CycleOutcome,
    val error: LastErrorCategory?,
)

/**
 * Persistence boundary for the result of the last completed backup cycle (`FR-AND-045 / 1`).
 *
 * The record is small and replaces the previous one, so a store keeps exactly one cycle result.
 */
internal interface LastCycleStore {
    /** Loads the recorded cycle, or nothing when no valid record is stored. */
    fun read(): LastCycleRecord?

    /** Replaces the recorded cycle with [record]. */
    fun write(record: LastCycleRecord)
}

/**
 * [LastCycleStore] that keeps the last cycle in one UTF-8 text file.
 *
 * The file holds three `key=value` lines in the order `completedAt`, `outcome`, `error`, where
 * `error` is empty for a successful cycle. A trailing newline is accepted and ignored. Reading an
 * absent, unreadable, or malformed file yields no record instead of failing, so a damaged file only
 * hides the last result (`FR-AND-045 / 1`). A write builds a temporary sibling and renames it over
 * the target, using an atomic move where the file system supports it, so a partial record is not
 * read back. The
 * caller owns the directory; a missing directory is not created. Not thread-safe; access must stay
 * on one thread or be externally synchronized.
 */
internal class FileLastCycleStore(
    private val file: File,
) : LastCycleStore {
    override fun read(): LastCycleRecord? {
        val content =
            try {
                if (!file.exists()) {
                    return null
                }
                file.readText(Charsets.UTF_8)
            } catch (_: Exception) {
                return null
            }
        return parse(content)
    }

    override fun write(record: LastCycleRecord) {
        val target = file.absoluteFile
        val directory =
            target.parentFile
                ?: throw IOException("Could not resolve the last cycle store directory")
        val temporary =
            try {
                File.createTempFile(TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, directory)
            } catch (cause: IOException) {
                throw IOException("Could not create a temporary last cycle store", cause)
            }
        try {
            temporary.writeText(format(record), Charsets.UTF_8)
            moveOver(temporary, target)
        } catch (cause: IOException) {
            throw IOException("Could not write the last cycle store", cause)
        } finally {
            if (temporary.exists()) {
                temporary.delete()
            }
        }
    }

    private fun format(record: LastCycleRecord): String {
        val error = if (record.outcome == CycleOutcome.SUCCESS) null else record.error
        return (
            listOf(
                "$COMPLETED_AT_FIELD$FIELD_SEPARATOR${record.completedAtMillis}",
                "$OUTCOME_FIELD$FIELD_SEPARATOR${record.outcome.name}",
                "$ERROR_FIELD$FIELD_SEPARATOR${error?.name.orEmpty()}",
            ).joinToString(separator = NEWLINE) + NEWLINE
        )
    }

    private fun parse(content: String): LastCycleRecord? {
        val body = if (content.endsWith(NEWLINE)) content.dropLast(NEWLINE.length) else content
        val lines = body.split(NEWLINE)
        val fields = mutableMapOf<String, String>()
        lines.forEach { line ->
            val separator = line.indexOf(FIELD_SEPARATOR)
            if (separator <= 0) {
                return null
            }
            fields[line.substring(0, separator)] = line.substring(separator + 1)
        }
        if (fields.size != lines.size || fields.keys != REQUIRED_FIELDS) {
            return null
        }
        val completedAtMillis = fields.getValue(COMPLETED_AT_FIELD).toLongOrNull() ?: return null
        val outcome =
            CycleOutcome.entries.firstOrNull { candidate ->
                candidate.name == fields.getValue(OUTCOME_FIELD)
            } ?: return null
        val errorField = fields.getValue(ERROR_FIELD)
        val error =
            if (errorField.isEmpty()) {
                null
            } else {
                LastErrorCategory.entries.firstOrNull { candidate ->
                    candidate.name == errorField
                } ?: return null
            }
        if (outcome == CycleOutcome.SUCCESS && error != null) {
            return null
        }
        return LastCycleRecord(
            completedAtMillis = completedAtMillis,
            outcome = outcome,
            error = error,
        )
    }

    private fun moveOver(
        source: File,
        target: File,
    ) {
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
        const val COMPLETED_AT_FIELD = "completedAt"
        const val OUTCOME_FIELD = "outcome"
        const val ERROR_FIELD = "error"

        val REQUIRED_FIELDS = setOf(COMPLETED_AT_FIELD, OUTCOME_FIELD, ERROR_FIELD)

        const val FIELD_SEPARATOR = "="
        const val NEWLINE = "\n"

        const val TEMPORARY_FILE_PREFIX = "last-cycle-"
        const val TEMPORARY_FILE_SUFFIX = ".tmp"
    }
}
