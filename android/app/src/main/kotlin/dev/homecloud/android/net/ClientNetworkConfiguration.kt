package dev.homecloud.android.net

/**
 * Validated network settings for one client configuration. Every value is configuration input: the
 * constructor defaults are the client values fixed by `CFG-CL-007`, `CFG-CL-027`,
 * `CFG-CL-017 / 2`, `CFG-CL-008 / 3`, `CFG-CL-018 / 2`, `CFG-CL-011` and `CFG-CL-031`, and each
 * value is checked against its requirement range when the configuration is created.
 *
 * An out-of-range value is a configuration error rather than a value to clamp: construction fails
 * with [IllegalArgumentException] that names the offending field.
 */
internal data class ClientNetworkConfiguration(
    val connectTimeoutMillis: Int = 10_000,
    val operationTimeoutMillis: Int = 30_000,
    val bootstrapMaxAttempts: Int = 5,
    val bootstrapRetryIntervalMillis: Long = 1_000,
    val maxFrameBytes: Int = 1_048_576,
    val maxLabelBytes: Int = 128,
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
