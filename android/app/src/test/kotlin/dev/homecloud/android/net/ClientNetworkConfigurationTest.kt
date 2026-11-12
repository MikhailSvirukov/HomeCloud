package dev.homecloud.android.net

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ClientNetworkConfigurationTest {
    @Test
    fun defaultsMatchTheDocumentedConfiguration() {
        val configuration = ClientNetworkConfiguration()

        assertEquals(10_000, configuration.connectTimeoutMillis)
        assertEquals(30_000, configuration.operationTimeoutMillis)
        assertEquals(5, configuration.bootstrapMaxAttempts)
        assertEquals(1_000L, configuration.bootstrapRetryIntervalMillis)
        assertEquals(4_194_304, configuration.maxFrameBytes)
        assertEquals(256, configuration.maxLabelBytes)
    }

    @Test
    fun connectTimeoutAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(connectTimeoutMillis = 1_000)
        val maximum = ClientNetworkConfiguration(connectTimeoutMillis = 60_000)

        assertEquals(1_000, minimum.connectTimeoutMillis)
        assertEquals(60_000, maximum.connectTimeoutMillis)
    }

    @Test
    fun operationTimeoutAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(operationTimeoutMillis = 1_000)
        val maximum = ClientNetworkConfiguration(operationTimeoutMillis = 300_000)

        assertEquals(1_000, minimum.operationTimeoutMillis)
        assertEquals(300_000, maximum.operationTimeoutMillis)
    }

    @Test
    fun bootstrapMaxAttemptsAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(bootstrapMaxAttempts = 1)
        val maximum = ClientNetworkConfiguration(bootstrapMaxAttempts = 10)

        assertEquals(1, minimum.bootstrapMaxAttempts)
        assertEquals(10, maximum.bootstrapMaxAttempts)
    }

    @Test
    fun bootstrapRetryIntervalAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(bootstrapRetryIntervalMillis = 100L)
        val maximum = ClientNetworkConfiguration(bootstrapRetryIntervalMillis = 60_000L)

        assertEquals(100L, minimum.bootstrapRetryIntervalMillis)
        assertEquals(60_000L, maximum.bootstrapRetryIntervalMillis)
    }

    @Test
    fun maxFrameBytesAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(maxFrameBytes = 4_096)
        val maximum = ClientNetworkConfiguration(maxFrameBytes = 67_108_864)

        assertEquals(4_096, minimum.maxFrameBytes)
        assertEquals(67_108_864, maximum.maxFrameBytes)
    }

    @Test
    fun maxLabelBytesAcceptsMinimumAndMaximum() {
        val minimum = ClientNetworkConfiguration(maxLabelBytes = 1)
        val maximum = ClientNetworkConfiguration(maxLabelBytes = 4_096)

        assertEquals(1, minimum.maxLabelBytes)
        assertEquals(4_096, maximum.maxLabelBytes)
    }

    @Test
    fun connectTimeoutBelowMinimumIsRejected() =
        assertRejected(CONNECT_TIMEOUT_FIELD) {
            ClientNetworkConfiguration(connectTimeoutMillis = 999)
        }

    @Test
    fun connectTimeoutAboveMaximumIsRejected() =
        assertRejected(CONNECT_TIMEOUT_FIELD) {
            ClientNetworkConfiguration(connectTimeoutMillis = 60_001)
        }

    @Test
    fun operationTimeoutBelowMinimumIsRejected() =
        assertRejected(OPERATION_TIMEOUT_FIELD) {
            ClientNetworkConfiguration(operationTimeoutMillis = 999)
        }

    @Test
    fun operationTimeoutAboveMaximumIsRejected() =
        assertRejected(OPERATION_TIMEOUT_FIELD) {
            ClientNetworkConfiguration(operationTimeoutMillis = 300_001)
        }

    @Test
    fun bootstrapMaxAttemptsBelowMinimumIsRejected() =
        assertRejected(BOOTSTRAP_MAX_ATTEMPTS_FIELD) {
            ClientNetworkConfiguration(bootstrapMaxAttempts = 0)
        }

    @Test
    fun bootstrapMaxAttemptsAboveMaximumIsRejected() =
        assertRejected(BOOTSTRAP_MAX_ATTEMPTS_FIELD) {
            ClientNetworkConfiguration(bootstrapMaxAttempts = 11)
        }

    @Test
    fun bootstrapRetryIntervalBelowMinimumIsRejected() =
        assertRejected(BOOTSTRAP_RETRY_INTERVAL_FIELD) {
            ClientNetworkConfiguration(bootstrapRetryIntervalMillis = 99L)
        }

    @Test
    fun bootstrapRetryIntervalAboveMaximumIsRejected() =
        assertRejected(BOOTSTRAP_RETRY_INTERVAL_FIELD) {
            ClientNetworkConfiguration(bootstrapRetryIntervalMillis = 60_001L)
        }

    @Test
    fun maxFrameBytesBelowMinimumIsRejected() =
        assertRejected(MAX_FRAME_BYTES_FIELD) {
            ClientNetworkConfiguration(maxFrameBytes = 4_095)
        }

    @Test
    fun maxFrameBytesAboveMaximumIsRejected() =
        assertRejected(MAX_FRAME_BYTES_FIELD) {
            ClientNetworkConfiguration(maxFrameBytes = 67_108_865)
        }

    @Test
    fun maxLabelBytesBelowMinimumIsRejected() =
        assertRejected(MAX_LABEL_BYTES_FIELD) {
            ClientNetworkConfiguration(maxLabelBytes = 0)
        }

    @Test
    fun maxLabelBytesAboveMaximumIsRejected() =
        assertRejected(MAX_LABEL_BYTES_FIELD) {
            ClientNetworkConfiguration(maxLabelBytes = 4_097)
        }

    @Test
    fun timeoutsMapsTheConfiguredTimeouts() {
        val configuration =
            ClientNetworkConfiguration(
                connectTimeoutMillis = 2_500,
                operationTimeoutMillis = 45_000,
            )

        val timeouts = configuration.timeouts()

        assertEquals(2_500, timeouts.connectMillis)
        assertEquals(45_000, timeouts.readMillis)
    }

    private fun assertRejected(
        field: String,
        configuration: () -> Unit,
    ) {
        val exception = assertThrows(IllegalArgumentException::class.java) { configuration() }

        assertTrue(exception.message.orEmpty().contains(field))
    }
}

private const val CONNECT_TIMEOUT_FIELD = "connectTimeoutMillis"
private const val OPERATION_TIMEOUT_FIELD = "operationTimeoutMillis"
private const val BOOTSTRAP_MAX_ATTEMPTS_FIELD = "bootstrapMaxAttempts"
private const val BOOTSTRAP_RETRY_INTERVAL_FIELD = "bootstrapRetryIntervalMillis"
private const val MAX_FRAME_BYTES_FIELD = "maxFrameBytes"
private const val MAX_LABEL_BYTES_FIELD = "maxLabelBytes"
