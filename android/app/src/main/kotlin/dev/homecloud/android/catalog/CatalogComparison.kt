package dev.homecloud.android.catalog

import dev.homecloud.android.media.MediaVersionIdentity

/**
 * Selects the local versions that the confirmed server catalog does not yet contain
 * (`FR-CAT-003 / 1`, `FR-CAT-004 / 1`).
 *
 * A local [MediaVersionIdentity] matches a [CatalogEntry] when the catalog entry's 32-byte SHA-256,
 * rendered as lowercase hexadecimal, equals the identity's content hash, and its source name and
 * timestamp equal the identity's original name and tracked time. A difference in any of the three
 * fields selects the version for upload. Catalog sizes are derived metadata and do not participate.
 * The input order is preserved and a repeated identity appears once, because the three fields are
 * exactly the version identity (`FR-AND-015 / 1`).
 */
internal object CatalogComparison {
    /**
     * The local versions absent from [catalog], in input order, with repeated identities collapsed.
     */
    fun missingVersions(
        local: List<MediaVersionIdentity>,
        catalog: List<CatalogEntry>,
    ): List<MediaVersionIdentity> {
        val confirmed = catalog.map { entry -> entry.confirmedIdentityKey() }.toSet()
        return local
            .distinct()
            .filter { version -> version.catalogKey() !in confirmed }
    }

    private fun CatalogEntry.confirmedIdentityKey(): String = "${sha256.toLowercaseHex()}\u0000$sourceName\u0000$timestampMillis"

    private fun MediaVersionIdentity.catalogKey(): String = "$contentSha256\u0000$originalName\u0000$trackedTimeMillis"
}

private fun ByteArray.toLowercaseHex(): String {
    val digits = "0123456789abcdef"
    val builder = StringBuilder(size * HEX_DIGITS_PER_BYTE)
    for (byte in this) {
        val value = byte.toInt() and BYTE_MASK
        builder.append(digits[value ushr BITS_PER_HEX_DIGIT])
        builder.append(digits[value and HEX_DIGIT_MASK])
    }
    return builder.toString()
}

private const val HEX_DIGITS_PER_BYTE = 2
private const val BITS_PER_HEX_DIGIT = 4
private const val HEX_DIGIT_MASK = 0x0F
private const val BYTE_MASK = 0xFF
