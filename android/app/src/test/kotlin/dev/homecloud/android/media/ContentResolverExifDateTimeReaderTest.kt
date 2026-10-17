package dev.homecloud.android.media

import android.content.ContentResolver
import android.net.Uri
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.io.IOException

/** Android 11, the API level the area's other Robolectric framework tests use. */
private const val TESTED_API = 30

/**
 * Exercises [ContentResolverExifDateTimeReader]'s failure handling through the platform resolver.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [TESTED_API])
@ConscryptMode(ConscryptMode.Mode.OFF)
class ContentResolverExifDateTimeReaderTest {
    @Test
    fun `read returns null when opening the stream fails`() {
        val resolver = resolver()
        shadowOf(resolver).registerInputStreamSupplier(Uri.parse(UNREADABLE_URI)) {
            throw IOException("unreadable stream")
        }

        assertNull(ContentResolverExifDateTimeReader(resolver).read(UNREADABLE_URI))
    }

    @Test
    @Config(shadows = [MissingStreamContentResolverShadow::class])
    fun `read returns null when the resolver yields no stream`() {
        assertNull(ContentResolverExifDateTimeReader(resolver()).read(MISSING_STREAM_URI))
    }

    private fun resolver(): ContentResolver = RuntimeEnvironment.getApplication().contentResolver

    private companion object {
        const val UNREADABLE_URI = "content://media/external/images/media/41"
        const val MISSING_STREAM_URI = "content://media/external/images/media/43"
    }
}
