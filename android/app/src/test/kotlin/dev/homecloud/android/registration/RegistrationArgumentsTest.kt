package dev.homecloud.android.registration

import dev.homecloud.android.protocol.CborDecoder
import dev.homecloud.android.protocol.CborValue
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/**
 * Asserts the registration operation arguments of `INT-NET-058 / 1`: a CBOR array of exactly two
 * 32-byte byte strings, with the installation X25519 public key first and the installation Ed25519
 * public key second.
 */
class RegistrationArgumentsTest {
    @Test
    fun `encode writes an array of exactly two items`() {
        val items = decodedItems(RegistrationArguments.encode(X25519_PUBLIC_KEY, ED25519_PUBLIC_KEY))

        assertEquals(ITEM_COUNT, items.size)
    }

    @Test
    fun `encode writes each key as a 32-byte byte string`() {
        val items = decodedItems(RegistrationArguments.encode(X25519_PUBLIC_KEY, ED25519_PUBLIC_KEY))

        assertEquals(listOf(PUBLIC_KEY_BYTES, PUBLIC_KEY_BYTES), items.map { item -> byteString(item).size })
    }

    @Test
    fun `encode writes the X25519 key before the Ed25519 key`() {
        val items = decodedItems(RegistrationArguments.encode(X25519_PUBLIC_KEY, ED25519_PUBLIC_KEY))

        assertArrayEquals(X25519_PUBLIC_KEY, byteString(items.first()))
        assertArrayEquals(ED25519_PUBLIC_KEY, byteString(items.last()))
    }

    @Test
    fun `encode rejects a 31-byte X25519 key`() {
        assertThrows(IllegalArgumentException::class.java) {
            RegistrationArguments.encode(ByteArray(PUBLIC_KEY_BYTES - 1), ED25519_PUBLIC_KEY)
        }
    }

    @Test
    fun `encode rejects a 33-byte X25519 key`() {
        assertThrows(IllegalArgumentException::class.java) {
            RegistrationArguments.encode(ByteArray(PUBLIC_KEY_BYTES + 1), ED25519_PUBLIC_KEY)
        }
    }

    @Test
    fun `encode rejects a 31-byte Ed25519 key`() {
        assertThrows(IllegalArgumentException::class.java) {
            RegistrationArguments.encode(X25519_PUBLIC_KEY, ByteArray(PUBLIC_KEY_BYTES - 1))
        }
    }

    private fun decodedItems(encoded: ByteArray): List<CborValue> =
        when (val value = CborDecoder.decode(encoded)) {
            is CborValue.Array -> value.items
            else -> throw AssertionError("registration arguments are not a CBOR array")
        }

    private fun byteString(value: CborValue): ByteArray =
        when (value) {
            is CborValue.Bytes -> value.value
            else -> throw AssertionError("registration argument is not a CBOR byte string")
        }

    private companion object {
        const val ITEM_COUNT = 2
        const val PUBLIC_KEY_BYTES = 32

        val X25519_PUBLIC_KEY = ByteArray(PUBLIC_KEY_BYTES) { index -> (index + 1).toByte() }
        val ED25519_PUBLIC_KEY = ByteArray(PUBLIC_KEY_BYTES) { index -> (index + 101).toByte() }
    }
}
