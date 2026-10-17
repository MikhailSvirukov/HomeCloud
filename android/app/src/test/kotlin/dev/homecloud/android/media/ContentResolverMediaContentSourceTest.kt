package dev.homecloud.android.media

import android.content.ContentResolver
import android.net.Uri
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import java.io.ByteArrayInputStream
import java.io.FileNotFoundException
import java.io.InputStream

/** Android 11, the API level the area's other Robolectric framework tests use. */
private const val TESTED_API = 30

/**
 * Exercises [ContentResolverMediaContentSource] through the platform resolver.
 *
 * Conscrypt is turned off because this test never touches TLS and loading its native library is
 * impossible on a `noexec` temp filesystem.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [TESTED_API])
@ConscryptMode(ConscryptMode.Mode.OFF)
class ContentResolverMediaContentSourceTest {
    @Test
    fun `open returns a stream over the registered content`() {
        val resolver = resolver()
        shadowOf(resolver).registerInputStream(Uri.parse(REGISTERED_URI), ByteArrayInputStream(CONTENT_BYTES))

        val opened = ContentResolverMediaContentSource(resolver).open(REGISTERED_URI)

        opened.use { stream ->
            assertArrayEquals(CONTENT_BYTES, stream.readBytes())
        }
    }

    @Test
    @Config(shadows = [MissingStreamContentResolverShadow::class])
    fun `open throws FileNotFoundException when the resolver yields no stream`() {
        assertThrows(FileNotFoundException::class.java) {
            ContentResolverMediaContentSource(resolver()).open(UNREGISTERED_URI)
        }
    }

    private fun resolver(): ContentResolver = RuntimeEnvironment.getApplication().contentResolver

    private companion object {
        const val REGISTERED_URI = "content://media/external/images/media/42"
        const val UNREGISTERED_URI = "content://media/external/images/media/43"
        val CONTENT_BYTES = "registered media bytes".toByteArray(Charsets.UTF_8)
    }
}

/**
 * Makes the resolver yield no stream for every open, so the source's missing-stream branch is
 * exercised instead of Robolectric's placeholder stream for unregistered content.
 */
@Implements(ContentResolver::class)
internal class MissingStreamContentResolverShadow {
    @Implementation
    fun openInputStream(uri: Uri): InputStream? = null
}
