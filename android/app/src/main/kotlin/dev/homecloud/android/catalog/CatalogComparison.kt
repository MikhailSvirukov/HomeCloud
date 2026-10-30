package dev.homecloud.android.catalog

/**
 * The identity of one local version: the content SHA-256, the source file name and the tracked
 * time (`FR-CAT-003 / 1`, `FR-CAT-004 / 1`).
 *
 * The digest is the 32-byte SHA-256 of the version's original bytes. Equality and hashing use the
 * digest content rather than the array instance, and the text form never contains the digest bytes.
 */
internal data class LocalVersion(
    val contentSha256: ByteArray,
    val originalName: String,
    val trackedTimeMillis: Long,
) {
    override fun toString(): String = "LocalVersion(originalName=$originalName, trackedTimeMillis=$trackedTimeMillis)"

    override fun equals(other: Any?): Boolean {
        if (this === other) {
            return true
        }
        if (other !is LocalVersion) {
            return false
        }
        return contentSha256.contentEquals(other.contentSha256) &&
            originalName == other.originalName &&
            trackedTimeMillis == other.trackedTimeMillis
    }

    override fun hashCode(): Int {
        var result = contentSha256.contentHashCode()
        result = 31 * result + originalName.hashCode()
        result = 31 * result + trackedTimeMillis.hashCode()
        return result
    }
}

/**
 * Selects the local versions that are absent from the confirmed catalog (`FR-CAT-003 / 1`,
 * `FR-CAT-004 / 1`).
 */
internal object CatalogComparison {
    /**
     * Produces the local versions the confirmed catalog does not already contain, preserving the
     * order in which they first appear locally.
     *
     * A local version matches a catalog entry when their content SHA-256, source name and timestamp
     * are equal. The catalog's size fields never participate, so entries whose sizes differ still
     * match. Repeated local identities produce one result because the three compared fields are the
     * version identity.
     */
    fun missingVersions(
        local: List<LocalVersion>,
        catalog: List<CatalogEntry>,
    ): List<LocalVersion> {
        val catalogIdentities =
            catalog.mapTo(HashSet()) { entry ->
                LocalVersion(
                    contentSha256 = entry.sha256,
                    originalName = entry.sourceName,
                    trackedTimeMillis = entry.timestampMillis,
                )
            }
        return local.distinct().filterNot(catalogIdentities::contains)
    }
}
