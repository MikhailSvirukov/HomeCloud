package dev.homecloud.android.net

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.InputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/**
 * Asserts the raw frame I/O of [TcpFrameChannel]: complete frames keep their four-byte length prefix,
 * one read consumes exactly one frame, and the configured limit and truncation are enforced
 * (`INT-NET-002`, `INT-NET-003`, `INT-NET-004`, `INT-NET-081`).
 */
class TcpFrameChannelTest {
    @Test
    fun `write delivers the exact frame bytes including the prefix`() {
        val body = byteArrayOf(0x01, 0x7F, 0x80.toByte(), 0xFF.toByte())
        val frame = lengthPrefix(body.size) + body

        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket -> readExactly(socket.getInputStream(), frame.size) }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                TcpFrameChannel(connection).use { channel -> channel.write(frame) }
            }

            assertArrayEquals(frame, serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS))
        }
    }

    @Test
    fun `read returns one frame and leaves the following frame`() {
        val firstFrame = lengthPrefix(FIRST_BODY.size) + FIRST_BODY
        val secondFrame = lengthPrefix(SECOND_BODY.size) + SECOND_BODY

        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        socket.getOutputStream().apply {
                            write(firstFrame)
                            write(secondFrame)
                            flush()
                        }
                    }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                TcpFrameChannel(connection).use { channel ->
                    assertArrayEquals(firstFrame, channel.read())
                    assertArrayEquals(secondFrame, channel.read())
                }
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        }
    }

    @Test
    fun `read rejects a declared length above the limit from the prefix alone`() {
        withServerSocket { server ->
            val accepted = CountDownLatch(1)
            val release = CountDownLatch(1)
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        socket.getOutputStream().apply {
                            write(lengthPrefix(SMALL_MAX_FRAME_BYTES + 1))
                            flush()
                        }
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
                    TcpFrameChannel(client).use { channel ->
                        assertTrue(accepted.await(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS))

                        assertThrows(ProtocolTransportException::class.java) { channel.read() }
                    }
                }
            } finally {
                release.countDown()
                serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            }
        }
    }

    @Test
    fun `read rejects a short frame`() {
        withServerSocket { server ->
            val serverTask =
                runInBackground {
                    server.accept().use { socket ->
                        socket.getOutputStream().apply {
                            write(lengthPrefix(SHORT_BODY_SIZE))
                            write(byteArrayOf(0x01, 0x02))
                            flush()
                        }
                    }
                }

            TcpProtocolConnection.connect(LOOPBACK, server.localPort).use { connection ->
                TcpFrameChannel(connection).use { channel ->
                    assertThrows(ProtocolTransportException::class.java) { channel.read() }
                }
            }

            serverTask.get(TEST_TIMEOUT_SECONDS, TimeUnit.SECONDS)
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

    private companion object {
        const val LOOPBACK = "127.0.0.1"
        const val SERVER_BACKLOG = 1
        const val SERVER_THREAD_NAME = "tcp-frame-channel-test"
        const val TEST_TIMEOUT_SECONDS = 10L
        const val SMALL_MAX_FRAME_BYTES = 8
        const val SHORT_BODY_SIZE = 4
        const val BITS_PER_BYTE = 8
        const val TWO_BYTES = 2
        const val THREE_BYTES = 3

        val FIRST_BODY = byteArrayOf(0x10, 0x20, 0x30)
        val SECOND_BODY = byteArrayOf(0x40, 0x50, 0x60, 0x70, 0x80.toByte())
    }
}
