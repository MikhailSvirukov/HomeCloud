package dev.homecloud.android.crypto

/**
 * Reports that a cryptographic input is malformed or that an authentication check failed.
 *
 * Messages identify the failing operation and never contain key material, plaintext or other
 * caller data.
 */
internal class CryptoException(
    message: String,
) : Exception(message)
