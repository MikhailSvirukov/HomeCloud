package dev.picbackup.android.backup

import dev.picbackup.android.media.MediaItem

/**
 * Supplies the opaque stable upload id for a backup candidate.
 *
 * The identity and versioning policy — what counts as one version of a file and what counts as a
 * new version — is deliberately not decided here. The caller injects that policy, and the id's
 * wire format is owned by the protocol contract. This seam neither generates nor parses the id;
 * it only asks the injected policy for the id of a candidate.
 */
fun interface UploadIdFactory {

    /**
     * Returns the opaque upload id for [item].
     *
     * @param item the discovered backup candidate to identify.
     * @return the stable upload id for the candidate's file version.
     */
    fun uploadIdFor(item: MediaItem): String
}
