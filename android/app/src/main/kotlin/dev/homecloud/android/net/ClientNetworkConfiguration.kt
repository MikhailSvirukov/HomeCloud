package dev.homecloud.android.net

/**
 * Validated network settings for one client configuration.
 *
 * The operation timeout, bootstrap attempt budget, retry interval, frame-body size and node-label
 * limit, together with their ranges, follow the shared configuration profile (`CFG-CL-017 / 2`,
 * `CFG-CL-008 / 3`, `CFG-CL-018 / 2`, `CFG-CL-011 / 1`, `CFG-CL-031 / 1`). The connect timeout is a
 * client default. The frame and label limits must match the server configuration.
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

        /** Default for the shared frame-body limit (`CFG-CL-011 / 1`); must match the server. */
        const val DEFAULT_MAX_FRAME_BYTES = 4_194_304

        /** Default for the shared node-label limit (`CFG-CL-031 / 1`); must match the server. */
        const val DEFAULT_MAX_LABEL_BYTES = 256
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
private val MAX_FRAME_BYTES_RANGE = 4_096..67_108_864
private val MAX_LABEL_BYTES_RANGE = 1..4_096
