package dev.homecloud.android.net

/**
 * Validated network settings for one client configuration.
 *
 * The operation timeout, bootstrap attempt budget and retry interval defaults and ranges are fixed
 * by the area SRS (`CFG-CL-017 / 2`, `CFG-CL-008 / 3`, `CFG-CL-018 / 2`). The connect timeout,
 * frame size and label size are client defaults for limits the shared design requires to be
 * configurable (`CFG-CL-011`, `CFG-CL-031`); the SRS does not fix their numeric values, and the
 * frame and label limits must match the server configuration.
 *
 * An out-of-range value is a configuration error rather than a value to clamp: construction fails
 * with [IllegalArgumentException] that names the offending field.
 */
internal data class ClientNetworkConfiguration(
    val connectTimeoutMillis: Int = DEFAULT_CONNECT_TIMEOUT_MILLIS,
    val operationTimeoutMillis: Int = DEFAULT_OPERATION_TIMEOUT_MILLIS,
    val bootstrapMaxAttempts: Int = DEFAULT_BOOTSTRAP_MAX_ATTEMPTS,
    val bootstrapRetryIntervalMillis: Long = DEFAULT_BOOTSTRAP_RETRY_INTERVAL_MILLIS,
    val maxFrameBytes: Int = DEFAULT_MAX_FRAME_BYTES,
    val maxLabelBytes: Int = DEFAULT_MAX_LABEL_BYTES,
) {
    init {
        require(connectTimeoutMillis in CONNECT_TIMEOUT_MILLIS_RANGE) {
            outOfRange("connectTimeoutMillis", connectTimeoutMillis, CONNECT_TIMEOUT_MILLIS_RANGE)
        }
        require(operationTimeoutMillis in OPERATION_TIMEOUT_MILLIS_RANGE) {
            outOfRange("operationTimeoutMillis", operationTimeoutMillis, OPERATION_TIMEOUT_MILLIS_RANGE)
        }
        require(bootstrapMaxAttempts in BOOTSTRAP_MAX_ATTEMPTS_RANGE) {
            outOfRange("bootstrapMaxAttempts", bootstrapMaxAttempts, BOOTSTRAP_MAX_ATTEMPTS_RANGE)
        }
        require(bootstrapRetryIntervalMillis in BOOTSTRAP_RETRY_INTERVAL_MILLIS_RANGE) {
            outOfRange(
                "bootstrapRetryIntervalMillis",
                bootstrapRetryIntervalMillis,
                BOOTSTRAP_RETRY_INTERVAL_MILLIS_RANGE,
            )
        }
        require(maxFrameBytes in MAX_FRAME_BYTES_RANGE) {
            outOfRange("maxFrameBytes", maxFrameBytes, MAX_FRAME_BYTES_RANGE)
        }
        require(maxLabelBytes in MAX_LABEL_BYTES_RANGE) {
            outOfRange("maxLabelBytes", maxLabelBytes, MAX_LABEL_BYTES_RANGE)
        }
    }

    /** Socket timeouts for a connection attempt and a storage-node operation, in milliseconds. */
    fun timeouts(): ConnectionTimeouts =
        ConnectionTimeouts(
            connectMillis = connectTimeoutMillis,
            readMillis = operationTimeoutMillis,
        )

    companion object {
        /** Client default for the connection attempt; the shared design fixes no numeric value. */
        const val DEFAULT_CONNECT_TIMEOUT_MILLIS = 10_000

        /** Client default for the storage-node operation wait (`CFG-CL-017 / 2`). */
        const val DEFAULT_OPERATION_TIMEOUT_MILLIS = 30_000

        /** Client default for the total bootstrap attempts including the first (`CFG-CL-008 / 3`). */
        const val DEFAULT_BOOTSTRAP_MAX_ATTEMPTS = 5

        /** Client default for the wait between bootstrap attempts (`CFG-CL-018 / 2`). */
        const val DEFAULT_BOOTSTRAP_RETRY_INTERVAL_MILLIS = 1_000L

        /** Client default for the shared frame limit (`CFG-CL-011`); must match the server. */
        const val DEFAULT_MAX_FRAME_BYTES = 1_048_576

        /** Client default for the shared label limit (`CFG-CL-031`); must match the server. */
        const val DEFAULT_MAX_LABEL_BYTES = 128
    }
}

private fun <T : Comparable<T>> outOfRange(
    field: String,
    value: T,
    range: ClosedRange<T>,
): String = "$field must be between ${range.start} and ${range.endInclusive} but was $value"

private val CONNECT_TIMEOUT_MILLIS_RANGE = 1_000..60_000
private val OPERATION_TIMEOUT_MILLIS_RANGE = 1_000..300_000
private val BOOTSTRAP_MAX_ATTEMPTS_RANGE = 1..10
private val BOOTSTRAP_RETRY_INTERVAL_MILLIS_RANGE = 100L..60_000L
private val MAX_FRAME_BYTES_RANGE = 1_024..16_777_216
private val MAX_LABEL_BYTES_RANGE = 1..1_024
