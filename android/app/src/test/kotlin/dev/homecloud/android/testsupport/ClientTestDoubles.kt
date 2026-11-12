package dev.homecloud.android.testsupport

import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityStore
import dev.homecloud.android.net.FrameChannel
import java.io.IOException
import java.nio.ByteBuffer
import java.util.UUID

/** In-memory identity store that always produces the same identity. */
internal class FixedIdentityStore(
    private val stored: InstallationIdentity,
) : InstallationIdentityStore {
    override fun load(): InstallationIdentity = stored

    override fun save(identity: InstallationIdentity) = Unit
}

/** In-memory identity store that holds no installation identity. */
internal class AbsentIdentityStore : InstallationIdentityStore {
    override fun load(): InstallationIdentity? = null

    override fun save(identity: InstallationIdentity) = Unit
}

/**
 * Scripted frame channel that records every written frame and answers a read with the response
 * built from the frames written so far.
 */
internal class RespondingFrameChannel(
    private val respond: (writes: List<ByteArray>) -> ByteArray,
) : FrameChannel {
    /** Copies of every frame written so far, in write order. */
    val writes = mutableListOf<ByteArray>()

    /** Whether [close] has been called. */
    var closed = false
        private set

    private var response: ByteArray? = null

    override fun write(frame: ByteArray) {
        writes += frame.copyOf()
        response = respond(writes)
    }

    override fun read(): ByteArray = response ?: throw AssertionError(NO_RESPONSE_MESSAGE)

    override fun close() {
        closed = true
    }

    private companion object {
        const val NO_RESPONSE_MESSAGE = "no response was scripted"
    }
}

/** Frame channel that records its close flag while every read fails with the scripted failure. */
internal class FailingReadFrameChannel(
    private val failure: IOException,
) : FrameChannel {
    /** Whether [close] has been called. */
    var closed = false
        private set

    override fun write(frame: ByteArray) = Unit

    override fun read(): ByteArray = throw failure

    override fun close() {
        closed = true
    }
}

/** Channel that the client must never use, so any use fails the test. */
internal fun unusedChannel(): FrameChannel = RespondingFrameChannel { throw AssertionError(CHANNEL_MUST_NOT_OPEN_MESSAGE) }

/** Returns the given value when present, otherwise fails the test with [message]. */
internal fun <T : Any> requireValue(
    value: T?,
    message: String,
): T = value ?: throw AssertionError(message)

/** Encodes UUID text into its sixteen-byte big-endian form. */
internal fun uuidBytes(value: String): ByteArray {
    val uuid = UUID.fromString(value)
    return ByteBuffer
        .allocate(UUID_BYTES)
        .putLong(uuid.mostSignificantBits)
        .putLong(uuid.leastSignificantBits)
        .array()
}

private const val UUID_BYTES = 16

private const val CHANNEL_MUST_NOT_OPEN_MESSAGE = "the channel must not be opened"
