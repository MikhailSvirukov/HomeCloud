package dev.homecloud.android.net

import dev.homecloud.android.protocol.FrameCodec
import dev.homecloud.android.protocol.ProtocolEncodingException
import java.io.Closeable
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException

/**
 * Reports that the v1 TCP transport could not be opened, used or closed (`INT-NET-001`). Messages
 * never contain payload bytes or credentials.
 */
internal class ProtocolTransportException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)

/**
 * Blocking socket timeouts in milliseconds. A connect or read timeout that is not positive is a
 * programming error and is rejected with [IllegalArgumentException].
 */
internal data class ConnectionTimeouts(
    val connectMillis: Int = ClientNetworkConfiguration.DEFAULT_CONNECT_TIMEOUT_MILLIS,
    val readMillis: Int = ClientNetworkConfiguration.DEFAULT_OPERATION_TIMEOUT_MILLIS,
) {
    init {
        require(connectMillis > 0) { "connectMillis must be positive but was $connectMillis" }
        require(readMillis > 0) { "readMillis must be positive but was $readMillis" }
    }
}

/**
 * A blocking, plain-JVM TCP connection that exchanges v1 length-prefixed frames (`INT-NET-001`,
 * `INT-NET-002`, `INT-NET-003`, `INT-NET-004`, `CFG-CL-011`).
 *
 * Payloads are opaque to the transport: the connection only adds or consumes the four-byte
 * big-endian length prefix and enforces the configured maximum frame size, so header and payload
 * encoding stay with the protocol layer. The connection performs blocking I/O and must not be used
 * on the main thread.
 *
 * A blank host, a port outside `1..65535` or a non-positive maximum frame size is rejected as a
 * programming error with [IllegalArgumentException] before any network operation starts.
 */
internal class TcpProtocolConnection private constructor(
    private val socket: Socket,
    private val maxFrameBytes: Int,
    private val readMillis: Int,
) : Closeable {
    private val input = socket.getInputStream()
    private val output = socket.getOutputStream()
    private val readLock = Any()
    private val writeLock = Any()
    private val closeLock = Any()

    @Volatile
    private var closed = false

    /**
     * Encodes `payload` as one frame, writes it and flushes the socket. A payload above the
     * configured limit, a closed connection or an I/O failure is reported as
     * [ProtocolTransportException].
     */
    fun send(payload: ByteArray) {
        synchronized(writeLock) {
            requireOpen()
            val frame =
                try {
                    FrameCodec.encode(payload, maxFrameBytes)
                } catch (exception: ProtocolEncodingException) {
                    throw ProtocolTransportException(SEND_REJECTED_MESSAGE, exception)
                }
            try {
                output.write(frame)
                output.flush()
            } catch (exception: IOException) {
                throw ProtocolTransportException(SEND_FAILED_MESSAGE, exception)
            }
        }
    }

    /**
     * Blocks until one complete frame arrives and returns only its payload. A truncated frame, a
     * declared length above the configured limit, a closed connection, a read timeout or another
     * I/O failure is reported as [ProtocolTransportException].
     */
    fun receive(): ByteArray {
        synchronized(readLock) {
            requireOpen()
            return try {
                FrameCodec.readFrom(input, maxFrameBytes)
            } catch (exception: SocketTimeoutException) {
                throw ProtocolTransportException("$READ_TIMEOUT_MESSAGE after $readMillis ms", exception)
            } catch (exception: ProtocolEncodingException) {
                throw ProtocolTransportException(RECEIVE_FAILED_MESSAGE, exception)
            } catch (exception: IOException) {
                throw ProtocolTransportException(RECEIVE_FAILED_MESSAGE, exception)
            }
        }
    }

    /** Closes the socket. Repeated calls have no further effect. */
    override fun close() {
        synchronized(closeLock) {
            if (closed) {
                return
            }
            closed = true
            try {
                socket.close()
            } catch (exception: IOException) {
                throw ProtocolTransportException(CLOSE_FAILED_MESSAGE, exception)
            }
        }
    }

    private fun requireOpen() {
        if (closed || socket.isClosed) {
            throw ProtocolTransportException(CONNECTION_CLOSED_MESSAGE)
        }
    }

    companion object {
        /**
         * Resolves `host` and `port` and opens a TCP connection with the connect timeout, the read
         * timeout and TCP_NODELAY set. Resolution or connection failures are reported as
         * [ProtocolTransportException].
         */
        fun connect(
            host: String,
            port: Int,
            timeouts: ConnectionTimeouts = ConnectionTimeouts(),
            maxFrameBytes: Int = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES,
        ): TcpProtocolConnection {
            require(host.isNotBlank()) { "host must not be blank" }
            require(port in MIN_PORT..MAX_PORT) { "port must be between 1 and 65535 but was $port" }
            require(maxFrameBytes > 0) {
                "maxFrameBytes must be positive but was $maxFrameBytes"
            }
            val socket = Socket()
            try {
                socket.connect(InetSocketAddress(host, port), timeouts.connectMillis)
                socket.tcpNoDelay = true
                socket.soTimeout = timeouts.readMillis
                return TcpProtocolConnection(socket, maxFrameBytes, timeouts.readMillis)
            } catch (exception: IOException) {
                closeAfterFailedConnect(socket)
                throw ProtocolTransportException(CONNECT_FAILED_MESSAGE, exception)
            } catch (exception: IllegalArgumentException) {
                closeAfterFailedConnect(socket)
                throw ProtocolTransportException(CONNECT_FAILED_MESSAGE, exception)
            }
        }

        private fun closeAfterFailedConnect(socket: Socket) {
            try {
                socket.close()
            } catch (_: IOException) {
                // The connect failure is the reported cause; cleanup failure cannot change it.
            }
        }
    }
}

private const val MIN_PORT = 1
private const val MAX_PORT = 65_535
private const val CONNECT_FAILED_MESSAGE = "could not establish the TCP connection"
private const val CONNECTION_CLOSED_MESSAGE = "connection is closed"
private const val SEND_REJECTED_MESSAGE = "frame could not be encoded for the connection"
private const val SEND_FAILED_MESSAGE = "could not send the frame"
private const val RECEIVE_FAILED_MESSAGE = "could not receive a frame"
private const val READ_TIMEOUT_MESSAGE = "frame read timed out"
private const val CLOSE_FAILED_MESSAGE = "could not close the TCP connection"
