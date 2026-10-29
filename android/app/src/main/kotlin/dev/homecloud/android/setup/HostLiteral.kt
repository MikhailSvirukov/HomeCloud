package dev.homecloud.android.setup

/**
 * Recognises the canonical host literals v1 permits (`FR-AND-040 / 2`, `INT-NET-070 / 5`): IPv4
 * with four decimal octets and no leading zeros, or IPv6 in RFC 5952 text form. DNS names, zone
 * identifiers, mixed IPv4-in-IPv6 notation and surrounding whitespace are rejected, so a setup
 * address is always one unambiguous literal. The check is pure and never resolves a name.
 */
internal object HostLiteral {
    fun isCanonical(host: String): Boolean =
        when {
            host.isEmpty() -> false
            host.any(Char::isWhitespace) -> false
            host.contains(COLON) -> isCanonicalIpv6(host)
            host.contains(DOT) -> isCanonicalIpv4(host)
            else -> false
        }

    private fun isCanonicalIpv4(host: String): Boolean {
        val octets = host.split(DOT)
        if (octets.size != IPV4_OCTET_COUNT) return false
        return octets.all(::isCanonicalIpv4Octet)
    }

    private fun isCanonicalIpv4Octet(octet: String): Boolean {
        if (octet.isEmpty() || octet.any { it !in DECIMAL_DIGITS }) return false
        if (octet.length > 1 && octet.first() == ZERO_DIGIT) return false
        val value = octet.toIntOrNull() ?: return false
        return value in MIN_OCTET..MAX_OCTET
    }

    private fun isCanonicalIpv6(host: String): Boolean {
        val groups = parseIpv6(host) ?: return false
        return renderIpv6(groups) == host
    }

    private fun parseIpv6(host: String): IntArray? {
        if (!host.all(::isIpv6Character)) return null
        val compression = host.indexOf(COMPRESSED_ZEROS)
        if (compression < 0) {
            val groups = parseGroups(host.split(COLON)) ?: return null
            return if (groups.size == IPV6_GROUP_COUNT) groups.toIntArray() else null
        }
        if (host.indexOf(COMPRESSED_ZEROS, compression + COMPRESSED_ZEROS.length) >= 0) return null
        val leftText = host.substring(0, compression)
        val rightText = host.substring(compression + COMPRESSED_ZEROS.length)
        val left = if (leftText.isEmpty()) emptyList() else parseGroups(leftText.split(COLON)) ?: return null
        val right = if (rightText.isEmpty()) emptyList() else parseGroups(rightText.split(COLON)) ?: return null
        if (left.size + right.size >= IPV6_GROUP_COUNT) return null
        val groups = IntArray(IPV6_GROUP_COUNT)
        left.forEachIndexed { index, group -> groups[index] = group }
        right.forEachIndexed { index, group -> groups[IPV6_GROUP_COUNT - right.size + index] = group }
        return groups
    }

    private fun parseGroups(parts: List<String>): List<Int>? = parts.map { part -> parseGroup(part) ?: return null }

    private fun parseGroup(group: String): Int? {
        if (group.isEmpty() || group.length > IPV6_GROUP_DIGITS) return null
        if (!group.all(::isLowercaseHexDigit)) return null
        return group.toIntOrNull(HEX_RADIX)
    }

    private fun renderIpv6(groups: IntArray): String {
        val run = longestZeroRun(groups)
        val builder = StringBuilder()
        var index = 0
        while (index < groups.size) {
            if (index == run.start) {
                builder.append(COMPRESSED_ZEROS)
                index += run.length
            } else {
                if (builder.isNotEmpty() && builder.last() != COLON) builder.append(COLON)
                builder.append(groups[index].toString(HEX_RADIX))
                index += 1
            }
        }
        return builder.toString()
    }

    private fun longestZeroRun(groups: IntArray): ZeroRun {
        var best = ZeroRun(NO_ZERO_RUN, 0)
        var index = 0
        while (index < groups.size) {
            if (groups[index] != 0) {
                index += 1
                continue
            }
            val start = index
            while (index < groups.size && groups[index] == 0) index += 1
            val length = index - start
            if (length >= MIN_COMPRESSED_ZEROS && length > best.length) best = ZeroRun(start, length)
        }
        return best
    }

    private fun isIpv6Character(character: Char): Boolean = character == COLON || isLowercaseHexDigit(character)

    private fun isLowercaseHexDigit(character: Char): Boolean = character in DECIMAL_DIGITS || character in LOWERCASE_HEX_LETTERS

    private data class ZeroRun(
        val start: Int,
        val length: Int,
    )

    private const val COLON = ':'
    private const val DOT = '.'
    private const val ZERO_DIGIT = '0'
    private const val COMPRESSED_ZEROS = "::"

    private const val IPV4_OCTET_COUNT = 4
    private const val MIN_OCTET = 0
    private const val MAX_OCTET = 255

    private const val IPV6_GROUP_COUNT = 8
    private const val IPV6_GROUP_DIGITS = 4
    private const val MIN_COMPRESSED_ZEROS = 2
    private const val NO_ZERO_RUN = -1
    private const val HEX_RADIX = 16

    private val DECIMAL_DIGITS = '0'..'9'
    private val LOWERCASE_HEX_LETTERS = 'a'..'f'
}
