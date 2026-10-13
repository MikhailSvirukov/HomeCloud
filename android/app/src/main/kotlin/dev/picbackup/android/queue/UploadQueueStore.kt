package dev.picbackup.android.queue

/**
 * Persistence boundary for the local upload queue.
 *
 * A store works on the complete snapshot: every write replaces everything previously stored, so the
 * queue domain model stays free of I/O and callers persist the whole queue instead of deltas.
 */
interface UploadQueueStore {

    /** Loads the stored snapshot in its persisted order. */
    fun read(): List<UploadQueueEntry>

    /** Replaces the stored snapshot with [entries]. */
    fun write(entries: List<UploadQueueEntry>)
}
