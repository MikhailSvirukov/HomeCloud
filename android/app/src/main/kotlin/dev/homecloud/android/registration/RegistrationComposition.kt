package dev.homecloud.android.registration

import android.content.Context
import dev.homecloud.android.identity.FileInstallationIdentityStore
import dev.homecloud.android.identity.KeystoreIdentityCipher
import dev.homecloud.android.net.ClientNetworkConfiguration
import dev.homecloud.android.net.ConnectionTimeouts
import dev.homecloud.android.net.TcpFrameChannel
import dev.homecloud.android.net.TcpProtocolConnection
import java.io.File

/**
 * Builds the production registration graph for the setup screen (`FR-AND-026 / 2`,
 * `FR-AND-029 / 1`, `FR-AND-053 / 1`).
 *
 * The installation identity lives in the app-private `installation-identity` file and the durable
 * pending registration in the app-private `pending-registration` file, both inside the app's files
 * directory. Both envelopes are protected by an Android KeyStore key that cannot be exported, so
 * private keys and the password never reach persistent storage in plaintext.
 *
 * Registration reads and writes those files and opens a blocking TCP connection, so the caller runs
 * [RegistrationController.register] off the main thread.
 */
internal object RegistrationComposition {
    /** App-private file that holds the encrypted installation identity. */
    internal const val IDENTITY_FILE_NAME = "installation-identity"

    /** App-private file that holds the encrypted pending registration. */
    internal const val PENDING_REGISTRATION_FILE_NAME = "pending-registration"

    /** Builds a controller whose stores and channel use the production files and network defaults. */
    fun controller(context: Context): RegistrationController {
        val cipher = KeystoreIdentityCipher()
        val identityStore =
            FileInstallationIdentityStore(
                file = File(context.filesDir, IDENTITY_FILE_NAME),
                cipher = cipher,
            )
        val pendingStore =
            FilePendingRegistrationStore(
                file = File(context.filesDir, PENDING_REGISTRATION_FILE_NAME),
                cipher = cipher,
            )
        val maxFrameBytes = ClientNetworkConfiguration.DEFAULT_MAX_FRAME_BYTES
        return RegistrationController(
            identityStore = identityStore,
            pendingStore = pendingStore,
            channelFactory = { host, port ->
                TcpFrameChannel(
                    TcpProtocolConnection.connect(
                        host = host,
                        port = port,
                        timeouts = ConnectionTimeouts(),
                        maxFrameBytes = maxFrameBytes,
                    ),
                )
            },
            maxFrameBytes = maxFrameBytes,
        )
    }
}
