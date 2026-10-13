package dev.picbackup.android.backup

import dev.picbackup.android.media.MediaItem

/**
 * Supplies the opaque stable upload id for a backup candidate.
 *
 * The identity and versioning policy is injected rather than decided here, and the id's wire format
 * belongs to the protocol contract; this seam neither generates nor parses it.
 */
fun interface UploadIdFactory {

    /** Returns the opaque upload id for a discovered candidate. */
    fun uploadIdFor(item: MediaItem): String
}
