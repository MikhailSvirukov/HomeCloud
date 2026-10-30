package dev.homecloud.android.net

import java.io.Closeable

/**
 * A duplex channel of complete application frames, each including its four-byte length prefix
 * (`INT-NET-002`, `INT-NET-003`, `INT-NET-081`).
 */
internal interface FrameChannel : Closeable {
    /** Writes one complete frame unchanged. */
    fun write(frame: ByteArray)

    /** Blocks until one complete frame arrives. */
    fun read(): ByteArray
}

/**
 * A [FrameChannel] over a blocking v1 TCP connection (`INT-NET-002`, `INT-NET-003`, `INT-NET-004`,
 * `INT-NET-081`).
 *
 * Closing the channel closes the underlying connection.
 */
internal class TcpFrameChannel(
    private val connection: TcpProtocolConnection,
) : FrameChannel {
    override fun write(frame: ByteArray) {
        connection.writeFrame(frame)
    }

    override fun read(): ByteArray = connection.readFrame()

    override fun close() {
        connection.close()
    }
}
