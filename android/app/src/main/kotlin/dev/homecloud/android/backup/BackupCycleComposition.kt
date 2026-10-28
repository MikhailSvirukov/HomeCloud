package dev.homecloud.android.backup

import android.content.Context
import dev.homecloud.android.log.AndroidBackupCycleLog
import dev.homecloud.android.media.ContentResolverExifDateTimeReader
import dev.homecloud.android.media.ContentResolverMediaContentSource
import dev.homecloud.android.media.ExifMediaMetadataTimeSource
import dev.homecloud.android.media.MediaStoreMediaRowSource
import dev.homecloud.android.media.MediaStoreVersionScanner
import dev.homecloud.android.queue.FileUploadQueueStore
import dev.homecloud.android.queue.InMemoryUploadQueue
import java.io.File
import java.time.ZoneId

/**
 * Builds the production backup cycle graph for one worker execution.
 *
 * The graph reads photos and videos from the device media store, hashes their original bytes,
 * resolves capture times in the device zone, and persists the upload queue outside the process so
 * pending work survives a restart or a worker stop (`FR-AND-016 / 1`, `CON-AND-002 / 1`,
 * `CON-AND-003 / 1`). A new graph is built per worker, so no cycle state is shared between
 * executions.
 */
internal object BackupCycleComposition {
    /**
     * Snapshot file inside the app's private storage. Keeping the queue on disk rather than only in
     * memory lets the next cycle restore pending uploads after the process ends.
     */
    private const val QUEUE_SNAPSHOT_FILE_NAME = "upload-queue.snapshot"

    /**
     * Attempts allowed for each queued upload before the queue marks it failed, so a persistently
     * unreachable endpoint does not retry forever.
     */
    private const val MAX_UPLOAD_ATTEMPTS = 3

    /**
     * Maximum number of versions the queue accepts, the configuration default for a large personal
     * library (`CFG-AND-002 / 2`).
     */
    private const val MAX_QUEUE_SIZE = 10_000

    fun runner(context: Context): BackupCycleRunner {
        val resolver = context.contentResolver
        val zone = ZoneId.systemDefault()
        val scanner =
            MediaStoreVersionScanner(
                MediaStoreMediaRowSource(resolver),
                ContentResolverMediaContentSource(resolver),
                ExifMediaMetadataTimeSource(ContentResolverExifDateTimeReader(resolver), zone),
                zone,
            )
        val queue = InMemoryUploadQueue(maxAttempts = MAX_UPLOAD_ATTEMPTS)
        val planner = MediaBackupPlanner(scanner, queue, maxQueueSize = MAX_QUEUE_SIZE)
        val store = FileUploadQueueStore(File(context.filesDir, QUEUE_SNAPSHOT_FILE_NAME))
        return BackupCycleRunner(planner, store, AndroidBackupCycleLog())
    }
}
