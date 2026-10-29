package dev.homecloud.android.net

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

class TcpProtocolConnectionTest {
    @Test
    fun sendWritesPayloadAsOneBigEndianFrame() {
        val payload = byteArrayOf(0x01, 0x02, 0x7F, 0x80.toByte(), 0xFF.toByte())

        withServerSocket { server ->
            val serverTask = runInBackground { readServerFrame(server) }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                connection.send(payload)
            }

            val frame = serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            assertArrayEquals(lengthPrefix(payload.size), frame.prefix)
            assertArrayEquals(payload, frame.payload)
        }
    }

    @Test
    fun receiveReturnsPayloadWrittenAsOneFrame() {
        val payload = byteArrayOf(0x10, 0x20, 0x30, 0x40)

        withServerSocket { server ->
            val serverTask = runInBackground { writeServerFrame(server, payload) }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                assertArrayEquals(payload, connection.receive())
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
    }

    @Test
    fun twoFramesInEachDirectionKeepTheirBoundaries() {
        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        val output = socket.getOutputStream()
                        writeFrame(output, FIRST_SERVER_PAYLOAD)
                        writeFrame(output, SECOND_SERVER_PAYLOAD)

                        val input = socket.getInputStream()
                        listOf(readFrame(input), readFrame(input))
                    }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                assertArrayEquals(FIRST_SERVER_PAYLOAD, connection.receive())
                assertArrayEquals(SECOND_SERVER_PAYLOAD, connection.receive())
                connection.send(FIRST_CLIENT_PAYLOAD)
                connection.send(SECOND_CLIENT_PAYLOAD)
            }

            val frames = serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            assertEquals(2, frames.size)
            assertArrayEquals(lengthPrefix(FIRST_CLIENT_PAYLOAD.size), frames[0].prefix)
            assertArrayEquals(FIRST_CLIENT_PAYLOAD, frames[0].payload)
            assertArrayEquals(lengthPrefix(SECOND_CLIENT_PAYLOAD.size), frames[1].prefix)
            assertArrayEquals(SECOND_CLIENT_PAYLOAD, frames[1].payload)
        }
    }

    @Test
    fun sendRejectsPayloadAboveConfiguredLimit() {
        val payload = ByteArray(SMALL_MAX_FRAME_BYTES + 1) { index -> index.toByte() }

        withServerSocket { server ->
            val accepted = CountDownLatch(1)
            val release = CountDownLatch(1)
            val serverTask =
                runInBackground {
                    server.accept().use {
                        accepted.countDown()
                        release.await(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
                    }
                }

            try {
                val connection =
                    TcpProtocolConnection.connect(
                        LOOPBACK,
                        server.localPort,
                        maxFrameBytes = SMALL_MAX_FRAME_BYTES,
                    )
                connection.use { client ->
                    assertTrue(accepted.await(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS))

                    assertThrows(ProtocolTransportException::class.java) { client.send(payload) }
                }
            } finally {
                release.countDown()
                serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            }
        }
    }

    @Test
    fun receiveRejectsDeclaredLengthAboveConfiguredLimit() {
        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        val output = socket.getOutputStream()
                        output.write(lengthPrefix(SMALL_MAX_FRAME_BYTES + 1))
                        output.flush()
                    }
                }

            val connection =
                TcpProtocolConnection.connect(
                    LOOPBACK,
                    server.localPort,
                    maxFrameBytes = SMALL_MAX_FRAME_BYTES,
                )
            connection.use { client ->
                assertThrows(ProtocolTransportException::class.java) { client.receive() }
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
    }

    @Test
    fun receiveRejectsTruncatedFrame() {
        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        val output = socket.getOutputStream()
                        output.write(lengthPrefix(TRUNCATED_PAYLOAD_SIZE))
                        output.write(byteArrayOf(0x01, 0x02))
                        output.flush()
                    }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                assertThrows(ProtocolTransportException::class.java) { connection.receive() }
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
    }

    @Test
    fun receiveTimesOutWhenServerKeepsConnectionOpenWithoutWriting() {
        withServerSocket { server ->
            val accepted = CountDownLatch(1)
            val release = CountDownLatch(1)
            val serverTask =
                runInBackground {
                    server.accept().use {
                        accepted.countDown()
                        release.await(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
                    }
                }

            try {
                val connection =
                    TcpProtocolConnection.connect(
                        LOOPBACK,
                        server.localPort,
                        ConnectionTimeouts(readMillis = SHORT_TIMEOUT_MILLIS),
                    )
                connection.use { client ->
                    assertTrue(accepted.await(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS))

                    val exception =
                        assertThrows(ProtocolTransportException::class.java) { client.receive() }
                    assertTrue(exception.message.orEmpty().contains("timed out"))
                }
            } finally {
                release.countDown()
                serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            }
        }
    }

    @Test
    fun connectToClosedPortThrowsProtocolTransportException() {
        val closedPort =
            ServerSocket(0, SERVER_BACKLOG, InetAddress.getByName(LOOPBACK)).use { it.localPort }

        assertThrows(ProtocolTransportException::class.java) {
            TcpProtocolConnection.connect(
                LOOPBACK,
                closedPort,
                ConnectionTimeouts(connectMillis = SHORT_TIMEOUT_MILLIS),
            )
        }
    }

    @Test
    fun closeIsIdempotentAndSendAfterCloseThrows() {
        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket -> socket.getInputStream().readBytes() }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                connection.close()
                connection.close()

                assertThrows(ProtocolTransportException::class.java) {
                    connection.send(FIRST_CLIENT_PAYLOAD)
                }
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
    }

    @Test
    fun connectRejectsInvalidHostPortAndFrameLimitAsProgrammingErrors() {
        assertThrows(IllegalArgumentException::class.java) {
            TcpProtocolConnection.connect("  ", VALID_PORT)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TcpProtocolConnection.connect(LOOPBACK, 0)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TcpProtocolConnection.connect(LOOPBACK, MAX_PORT + 1)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TcpProtocolConnection.connect(LOOPBACK, VALID_PORT, maxFrameBytes = 0)
        }
    }

    @Test
    fun connectionTimeoutsRejectNonPositiveValues() {
        assertThrows(IllegalArgumentException::class.java) {
            ConnectionTimeouts(connectMillis = 0)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ConnectionTimeouts(connectMillis = -1)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ConnectionTimeouts(readMillis = 0)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ConnectionTimeouts(readMillis = -1)
        }
    }

    private fun withServerSocket(block: (ServerSocket) -> Unit) {
        val server = ServerSocket(0, SERVER_BACKLOG, InetAddress.getByName(LOOPBACK))
        try {
            block(server)
        } finally {
            server.close()
        }
    }

    private fun <T> runInBackground(block: () -> T): FutureTask<T> {
        val task = FutureTask<T> { block() }
        Thread(task, SERVER_THREAD_NAME).apply {
            isDaemon = true
            start()
        }
        return task
    }

    private fun readServerFrame(server: ServerSocket): ServerFrame = server.accept().use { socket -> readFrame(socket.getInputStream()) }

    private fun writeServerFrame(
        server: ServerSocket,
        payload: ByteArray,
    ) {
        server.accept().use { socket ->
            val output = socket.getOutputStream()
            writeFrame(output, payload)
        }
    }

    private fun writeFrame(
        output: OutputStream,
        payload: ByteArray,
    ) {
        output.write(lengthPrefix(payload.size))
        output.write(payload)
        output.flush()
    }

    private fun readFrame(input: InputStream): ServerFrame {
        val prefix = readExactly(input, LENGTH_PREFIX_BYTES)
        val payload = readExactly(input, decodeLength(prefix))
        return ServerFrame(prefix, payload)
    }

    private fun readExactly(
        input: InputStream,
        size: Int,
    ): ByteArray {
        val bytes = ByteArray(size)
        var readBytes = 0
        while (readBytes < size) {
            val count = input.read(bytes, readBytes, size - readBytes)
            if (count < 0) {
                throw AssertionError("stream ended after $readBytes of $size bytes")
            }
            readBytes += count
        }
        return bytes
    }

    private fun lengthPrefix(length: Int): ByteArray =
        byteArrayOf(
            (length ushr (BITS_PER_BYTE * THREE_BYTES)).toByte(),
            (length ushr (BITS_PER_BYTE * TWO_BYTES)).toByte(),
            (length ushr BITS_PER_BYTE).toByte(),
            length.toByte(),
        )

    private fun decodeLength(prefix: ByteArray): Int {
        var length = 0
        for (index in 0 until LENGTH_PREFIX_BYTES) {
            length = (length shl BITS_PER_BYTE) or (prefix[index].toInt() and BYTE_MASK)
        }
        return length
    }

    private data class ServerFrame(
        val prefix: ByteArray,
        val payload: ByteArray,
    )

    private companion object {
        const val LOOPBACK = "127.0.0.1"
        const val SERVER_BACKLOG = 1
        const val SERVER_THREAD_NAME = "tcp-protocol-connection-test"
        const val TEST_TIMEOUT_SECONDS = 10L
        const val SHORT_TIMEOUT_MILLIS = 300
        const val SMALL_MAX_FRAME_BYTES = 8
        const val TRUNCATED_PAYLOAD_SIZE = 4
        const val LENGTH_PREFIX_BYTES = 4
        const val BITS_PER_BYTE = 8
        const val TWO_BYTES = 2
        const val THREE_BYTES = 3
        const val BYTE_MASK = 0xFF
        const val VALID_PORT = 1
        const val MAX_PORT = 65_535

        val FIRST_SERVER_PAYLOAD = byteArrayOf(0x01, 0x02, 0x03)
        val SECOND_SERVER_PAYLOAD = byteArrayOf(0x04, 0x05, 0x06, 0x07)
        val FIRST_CLIENT_PAYLOAD = byteArrayOf(0x11, 0x12)
        val SECOND_CLIENT_PAYLOAD = byteArrayOf(0x21, 0x22, 0x23)
    }
}
