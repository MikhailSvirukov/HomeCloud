package dev.homecloud.android.setup

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the host rule of `FR-AND-040 / 2` and `INT-NET-070 / 5`: only a canonical IPv4 or
 * RFC 5952 IPv6 literal names a setup address. The check is pure, so every scenario is a plain
 * string that the parser either recognises unchanged or rejects.
 */
class HostLiteralTest {
    @Test
    fun `canonical ipv4 literal is accepted`() {
        CANONICAL_IPV4_LITERALS.forEach { host ->
            assertTrue("expected $host to be canonical", HostLiteral.isCanonical(host))
        }
    }

    @Test
    fun `compressed canonical ipv6 literal is accepted`() {
        COMPRESSED_IPV6_LITERALS.forEach { host ->
            assertTrue("expected $host to be canonical", HostLiteral.isCanonical(host))
        }
    }

    @Test
    fun `full eight group ipv6 literal is accepted`() {
        assertTrue(HostLiteral.isCanonical(FULL_IPV6_LITERAL))
    }

    @Test
    fun `ipv4 literal with a non decimal octet is rejected`() {
        assertFalse(HostLiteral.isCanonical(NON_DECIMAL_OCTET_IPV4))
    }

    @Test
    fun `ipv4 literal with a leading zero octet is rejected`() {
        assertFalse(HostLiteral.isCanonical(LEADING_ZERO_IPV4))
    }

    @Test
    fun `ipv4 literal with an octet above 255 is rejected`() {
        assertFalse(HostLiteral.isCanonical(OUT_OF_RANGE_IPV4))
    }

    @Test
    fun `ipv4 literal with three octets is rejected`() {
        assertFalse(HostLiteral.isCanonical(THREE_OCTET_IPV4))
    }

    @Test
    fun `dns name is rejected`() {
        assertFalse(HostLiteral.isCanonical(DNS_NAME))
    }

    @Test
    fun `host with surrounding whitespace is rejected`() {
        assertFalse(HostLiteral.isCanonical(PADDED_IPV4))
    }

    @Test
    fun `uppercase ipv6 literal is rejected`() {
        assertFalse(HostLiteral.isCanonical(UPPERCASE_IPV6))
    }

    @Test
    fun `ipv6 literal with a zone identifier is rejected`() {
        assertFalse(HostLiteral.isCanonical(ZONE_ID_IPV6))
    }

    @Test
    fun `mixed ipv4 in ipv6 literal is rejected`() {
        assertFalse(HostLiteral.isCanonical(MIXED_IPV6))
    }

    @Test
    fun `ipv6 literal with two compressions is rejected`() {
        assertFalse(HostLiteral.isCanonical(TWO_COMPRESSIONS_IPV6))
    }

    private companion object {
        val CANONICAL_IPV4_LITERALS = listOf("0.0.0.0", "192.0.2.10", "255.255.255.255")
        val COMPRESSED_IPV6_LITERALS = listOf("::", "::1", "2001:db8::1")

        const val FULL_IPV6_LITERAL = "2001:db8:1:2:3:4:5:6"
        const val NON_DECIMAL_OCTET_IPV4 = "192.0.2.0x10"
        const val LEADING_ZERO_IPV4 = "192.0.02.10"
        const val OUT_OF_RANGE_IPV4 = "192.0.2.256"
        const val THREE_OCTET_IPV4 = "192.0.2"
        const val DNS_NAME = "example.com"
        const val PADDED_IPV4 = " 192.0.2.10"
        const val UPPERCASE_IPV6 = "2001:DB8::1"
        const val ZONE_ID_IPV6 = "fe80::1%eth0"
        const val MIXED_IPV6 = "::ffff:192.0.2.1"
        const val TWO_COMPRESSIONS_IPV6 = "2001:db8::1::2"
    }
}
