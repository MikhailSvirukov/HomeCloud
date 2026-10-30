package dev.homecloud.android.registration

import dev.homecloud.android.identity.DedicatedServerKeys
import dev.homecloud.android.identity.InstallationIdentity
import dev.homecloud.android.identity.InstallationIdentityFactory
import dev.homecloud.android.identity.InstallationIdentityStore
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asserts the durable primary-registration flow of `FR-AND-028 / 2`, `FR-AND-053 / 1`,
 * `FR-AND-054 / 1` and `FR-AND-055 / 1`: the pending record is written before the first send, a
 * transport or identity-save failure keeps it for the same logical request, a restart repeats the
 * stored request instead of generating a new identity, and a completed registration stores the
 * assigned id and clears the pending record.
 *
 * The identity store, pending store and transport are in-memory test doubles so the coordinator's
 * ordering and failure handling are observed without file I/O or a network.
 */
class RegistrationCoordinatorTest {
    private val identity =
        InstallationIdentityFactory.newIdentity().copy(
            dedicatedServerKeys = DedicatedServerKeys(ByteArray(32) { 1 }, ByteArray(32) { 2 }),
        )

    @Test
    fun `registration keeps the server keys pinned during the handshake`() {
        val store = FakeInstallationIdentityStore(identity.copy(dedicatedServerKeys = null))
        val serverKeys = DedicatedServerKeys(ByteArray(32) { 3 }, ByteArray(32) { 4 })
        val transport =
            FakeRegistrationTransport().apply {
                onRegister = {
                    val current = requireNotNull(store.stored)
                    store.stored = current.copy(dedicatedServerKeys = serverKeys)
                }
            }

        val outcome = coordinator(store, FakePendingRegistrationStore(), transport).register(LOGIN, PASSWORD)

        assertTrue(outcome is RegistrationOutcome.Registered)
        val saved = requireNotNull(store.stored)
        assertArrayEquals(serverKeys.x25519PublicKey, saved.dedicatedServerKeys?.x25519PublicKey)
        assertArrayEquals(serverKeys.ed25519PublicKey, saved.dedicatedServerKeys?.ed25519PublicKey)
    }

    @Test
    fun `missing identity reports NoIdentity without touching the stores or the transport`() {
        val identityStore = FakeInstallationIdentityStore(null)
        val pendingStore = FakePendingRegistrationStore()
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(identityStore, pendingStore, transport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.NoIdentity, outcome)
        assertEquals(0, pendingStore.loadCount)
        assertEquals(0, pendingStore.saveCount)
        assertEquals(0, pendingStore.clearCount)
        assertTrue(transport.requests.isEmpty())
    }

    @Test
    fun `pending record is stored before the transport is called`() {
        val pendingStore = FakePendingRegistrationStore()
        var pendingAtCall: PendingRegistration? = null
        val transport =
            FakeRegistrationTransport().apply {
                onRegister = { pendingAtCall = pendingStore.record }
            }

        coordinator(FakeInstallationIdentityStore(identity), pendingStore, transport).register(LOGIN, PASSWORD)

        val stored = requireValue(pendingAtCall, "transport was called before the pending record was saved")
        val request = transport.requests.single()
        assertArrayEquals(request.requestId, stored.requestId)
        assertEquals(LOGIN, stored.login)
        assertEquals(PASSWORD, stored.password)
        assertEquals(1, pendingStore.saveCount)
    }

    @Test
    fun `pending save failure reports StorageFailed without sending`() {
        val pendingStore = FakePendingRegistrationStore().apply { saveFailure = true }
        val identityStore = FakeInstallationIdentityStore(identity)
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(identityStore, pendingStore, transport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.StorageFailed, outcome)
        assertTrue(transport.requests.isEmpty())
        assertEquals(0, identityStore.saveCount)
        assertNull(pendingStore.record)
    }

    @Test
    fun `successful response binds the assigned id to the existing keys and clears the record`() {
        val identityStore = FakeInstallationIdentityStore(identity)
        val pendingStore = FakePendingRegistrationStore()
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(identityStore, pendingStore, transport).register(LOGIN, PASSWORD)

        val registered = outcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $outcome")
        assertArrayEquals(INSTALLATION_ID, registered.installationId)
        val savedIdentity = requireValue(identityStore.stored, "registered identity was not saved")
        assertArrayEquals(INSTALLATION_ID, requireValue(savedIdentity.senderId, "assigned id was not stored"))
        assertArrayEquals(identity.agreementKeyPair.publicKey, savedIdentity.agreementKeyPair.publicKey)
        assertArrayEquals(identity.agreementKeyPair.privateKey, savedIdentity.agreementKeyPair.privateKey)
        assertArrayEquals(identity.signingKeyPair.publicKey, savedIdentity.signingKeyPair.publicKey)
        assertArrayEquals(identity.signingKeyPair.privateKey, savedIdentity.signingKeyPair.privateKey)
        assertNull(pendingStore.record)
        assertEquals(1, transport.requests.size)
    }

    @Test
    fun `missing installation id reports Pending and keeps the record`() {
        val pendingStore = FakePendingRegistrationStore()
        val transport = FakeRegistrationTransport(response = RegistrationResponse(null))

        val outcome = coordinator(FakeInstallationIdentityStore(identity), pendingStore, transport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, outcome)
        val stored = requireValue(pendingStore.record, "pending record was cleared without an installation id")
        assertArrayEquals(transport.requests.single().requestId, stored.requestId)
        assertEquals(0, pendingStore.clearCount)
    }

    @Test
    fun `transport failure reports Pending and keeps the record`() {
        val pendingStore = FakePendingRegistrationStore()
        val transport =
            FakeRegistrationTransport(failure = RegistrationTransportException("registration transport failed"))

        val outcome = coordinator(FakeInstallationIdentityStore(identity), pendingStore, transport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, outcome)
        requireValue(pendingStore.record, "failed transport cleared the pending record")
        assertEquals(0, pendingStore.clearCount)
    }

    @Test
    fun `restart repeats the stored request without a new identity or request id`() {
        val identityStore = FakeInstallationIdentityStore(identity)
        val pendingStore = FakePendingRegistrationStore()
        val failedTransport =
            FakeRegistrationTransport(failure = RegistrationTransportException("registration transport failed"))

        val firstOutcome = coordinator(identityStore, pendingStore, failedTransport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, firstOutcome)
        val stored = requireValue(pendingStore.record, "first attempt did not store a pending record")
        val retryTransport = FakeRegistrationTransport()
        val retryOutcome = coordinator(identityStore, pendingStore, retryTransport).register(LOGIN, PASSWORD)

        val registered = retryOutcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $retryOutcome")
        assertArrayEquals(INSTALLATION_ID, registered.installationId)
        val request = retryTransport.requests.single()
        assertArrayEquals(stored.requestId, request.requestId)
        assertEquals(stored.login, request.login)
        assertArrayEquals(stored.arguments, request.arguments)
        assertArrayEquals(stored.x25519PublicKey, request.x25519PublicKey)
        assertArrayEquals(stored.ed25519PublicKey, request.ed25519PublicKey)
        assertEquals(1, identityStore.saveCount)
        val savedIdentity = requireValue(identityStore.stored, "registered identity was not saved")
        assertArrayEquals(identity.agreementKeyPair.publicKey, savedIdentity.agreementKeyPair.publicKey)
        assertArrayEquals(identity.signingKeyPair.publicKey, savedIdentity.signingKeyPair.publicKey)
    }

    @Test
    fun `identity with an assigned id clears a stale record without sending`() {
        val identityStore = FakeInstallationIdentityStore(identity.copy(senderId = REGISTERED_SENDER_ID))
        val pendingStore = FakePendingRegistrationStore(pendingRecord())
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(identityStore, pendingStore, transport).register(LOGIN, PASSWORD)

        val registered = outcome as? RegistrationOutcome.Registered ?: throw AssertionError("not registered: $outcome")
        assertArrayEquals(REGISTERED_SENDER_ID, registered.installationId)
        assertTrue(transport.requests.isEmpty())
        assertEquals(0, identityStore.saveCount)
        assertNull(pendingStore.record)
    }

    @Test
    fun `different login starts a new request with a new request id`() {
        val pendingStore = FakePendingRegistrationStore(pendingRecord(login = STORED_LOGIN))
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(FakeInstallationIdentityStore(identity), pendingStore, transport).register(LOGIN, PASSWORD)

        assertTrue(outcome is RegistrationOutcome.Registered)
        val request = transport.requests.single()
        assertFalse(STORED_REQUEST_ID.contentEquals(request.requestId))
        assertEquals(LOGIN, request.login)
        assertEquals(1, pendingStore.saveCount)
        assertNull(pendingStore.record)
    }

    @Test
    fun `identity save failure after a successful response keeps the record`() {
        val identityStore = FakeInstallationIdentityStore(identity).apply { saveFailure = true }
        val pendingStore = FakePendingRegistrationStore()
        val transport = FakeRegistrationTransport()

        val outcome = coordinator(identityStore, pendingStore, transport).register(LOGIN, PASSWORD)

        assertEquals(RegistrationOutcome.Pending, outcome)
        val stored = requireValue(pendingStore.record, "identity save failure cleared the pending record")
        assertArrayEquals(transport.requests.single().requestId, stored.requestId)
        assertEquals(0, pendingStore.clearCount)
    }

    private fun coordinator(
        identityStore: InstallationIdentityStore,
        pendingStore: PendingRegistrationStore,
        transport: RegistrationTransport,
    ): RegistrationCoordinator = RegistrationCoordinator(identityStore, pendingStore, transport) { STARTED_AT_MILLIS }

    private fun pendingRecord(login: String = LOGIN): PendingRegistration {
        val x25519PublicKey = identity.agreementKeyPair.publicKey
        val ed25519PublicKey = identity.signingKeyPair.publicKey
        return PendingRegistration(
            requestId = STORED_REQUEST_ID,
            login = login,
            password = STORED_PASSWORD,
            x25519PublicKey = x25519PublicKey,
            ed25519PublicKey = ed25519PublicKey,
            arguments = RegistrationArguments.encode(x25519PublicKey, ed25519PublicKey),
            startedAtMillis = STORED_STARTED_AT_MILLIS,
        )
    }

    private fun <T : Any> requireValue(
        value: T?,
        message: String,
    ): T = value ?: throw AssertionError(message)

    private companion object {
        const val LOGIN = "pending-login"
        const val STORED_LOGIN = "stored-login"
        const val PASSWORD = "pending-password"
        const val STORED_PASSWORD = "stored-password"
        const val STARTED_AT_MILLIS = 1_700_000_000_000L
        const val STORED_STARTED_AT_MILLIS = 1_699_000_000_000L

        val INSTALLATION_ID = ByteArray(16) { index -> (index + 1).toByte() }
        val REGISTERED_SENDER_ID = ByteArray(16) { index -> (index + 51).toByte() }
        val STORED_REQUEST_ID = ByteArray(16) { index -> (index + 101).toByte() }
    }
}

/** In-memory installation identity store that can refuse to save (`FR-AND-029 / 1`). */
private class FakeInstallationIdentityStore(
    var stored: InstallationIdentity?,
) : InstallationIdentityStore {
    var saveFailure = false
    var saveCount = 0

    override fun load(): InstallationIdentity? = stored

    override fun save(identity: InstallationIdentity) {
        if (saveFailure) throw IllegalStateException(SAVE_FAILURE_MESSAGE)
        saveCount += 1
        stored = identity
    }

    private companion object {
        const val SAVE_FAILURE_MESSAGE = "test identity store cannot save"
    }
}

/** In-memory pending store that records calls and can refuse to save (`FR-AND-053 / 1`). */
private class FakePendingRegistrationStore(
    var record: PendingRegistration? = null,
) : PendingRegistrationStore {
    var saveFailure = false
    var loadCount = 0
    var saveCount = 0
    var clearCount = 0

    override fun load(): PendingRegistration? {
        loadCount += 1
        return record
    }

    override fun save(record: PendingRegistration) {
        saveCount += 1
        if (saveFailure) throw IllegalStateException(SAVE_FAILURE_MESSAGE)
        this.record = record
    }

    override fun clear() {
        clearCount += 1
        record = null
    }

    private companion object {
        const val SAVE_FAILURE_MESSAGE = "test pending store cannot save"
    }
}

/** Registration transport double that records requests and can fail (`INT-NET-058 / 1`). */
private class FakeRegistrationTransport(
    var response: RegistrationResponse = RegistrationResponse(INSTALLATION_ID),
    var failure: Exception? = null,
) : RegistrationTransport {
    val requests = mutableListOf<RegistrationRequest>()
    var onRegister: ((RegistrationRequest) -> Unit)? = null

    override fun register(request: RegistrationRequest): RegistrationResponse {
        requests += request
        onRegister?.invoke(request)
        failure?.let { error -> throw error }
        return response
    }

    private companion object {
        val INSTALLATION_ID = ByteArray(16) { index -> (index + 1).toByte() }
    }
}
