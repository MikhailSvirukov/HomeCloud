package dev.homecloud.android.media

import dev.homecloud.android.testsupport.FakeMediaRowSource
import dev.homecloud.android.testsupport.MediaTestData
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.IOException
import java.io.InputStream
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset

class MediaStoreVersionScannerTest {
    @Test
    fun `only image and video rows become candidates in row id order`() {
        val imageRow = imageRow()
        val audioRow = MediaTestData.row(id = AUDIO_ID, mimeType = MediaTestData.AUDIO_MIME_TYPE)
        val videoRow = videoRow()

        val candidates =
            scanner(
                rows = FakeMediaRowSource(listOf(imageRow, audioRow, videoRow)),
                content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES, videoRow.uri to EMPTY_BYTES)),
            ).scan()

        assertEquals(listOf(IMAGE_ID, VIDEO_ID), candidates.map { it.item.id })
        assertEquals(listOf(MediaKind.IMAGE, MediaKind.VIDEO), candidates.map { it.item.kind })
    }

    @Test
    fun `identity carries the file SHA-256 the display name and the resolved tracked time`() {
        val imageRow = imageRow()
        val videoRow = videoRow()
        val metadataTime =
            RecordingMetadataTimeSource(
                mapOf(
                    imageRow.uri to FIXED_METADATA_TIME,
                    videoRow.uri to FIXED_METADATA_TIME,
                ),
            )

        val candidates =
            scanner(
                rows = FakeMediaRowSource(listOf(imageRow, videoRow)),
                content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES, videoRow.uri to EMPTY_BYTES)),
                metadataTime = metadataTime,
            ).scan()

        assertEquals(listOf(ABC_SHA256, EMPTY_SHA256), candidates.map { it.identity.contentSha256 })
        assertEquals(listOf(imageRow.displayName, videoRow.displayName), candidates.map { it.identity.originalName })
        assertEquals(
            listOf(FIXED_METADATA_EPOCH_MILLIS, FIXED_METADATA_EPOCH_MILLIS),
            candidates.map { it.identity.trackedTimeMillis },
        )
        val mimeTypeByUri = metadataTime.calls.associate { it.uri to it.mimeType }
        assertTrue(mimeTypeByUri.getValue(imageRow.uri).startsWith(IMAGE_MIME_PREFIX))
        assertTrue(mimeTypeByUri.getValue(videoRow.uri).startsWith(VIDEO_MIME_PREFIX))
    }

    @Test
    fun `the content stream opened for a candidate is closed after hashing`() {
        val imageRow = imageRow()
        val content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES))

        scanner(rows = FakeMediaRowSource(listOf(imageRow)), content = content).scan()

        assertTrue(content.opened.single().isClosed)
    }

    @Test
    fun `tracked time falls back to the row modification seconds when metadata has no time`() {
        val imageRow = imageRow(dateModifiedEpochSeconds = MODIFIED_EPOCH_SECONDS)

        val candidate =
            scanner(
                rows = FakeMediaRowSource(listOf(imageRow)),
                content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES)),
            ).scan().single()

        assertEquals(MODIFIED_EPOCH_MILLIS, candidate.identity.trackedTimeMillis)
    }

    @Test
    fun `zone-less metadata time is interpreted in the scanner device zone`() {
        val imageRow = imageRow()
        val metadataTime = RecordingMetadataTimeSource(mapOf(imageRow.uri to ZONE_LESS_METADATA_TIME))

        val candidate =
            scanner(
                rows = FakeMediaRowSource(listOf(imageRow)),
                content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES)),
                metadataTime = metadataTime,
            ).scan().single()

        assertEquals(BERLIN_SUMMER_EPOCH_MILLIS, candidate.identity.trackedTimeMillis)
    }

    @Test
    fun `scan reads the row source once per call`() {
        val imageRow = imageRow()
        val videoRow = videoRow()
        val rowSource = FakeMediaRowSource(listOf(imageRow, videoRow))
        val versionScanner =
            scanner(
                rows = rowSource,
                content = FakeContentSource(mapOf(imageRow.uri to ABC_BYTES, videoRow.uri to EMPTY_BYTES)),
            )

        versionScanner.scan()
        versionScanner.scan()

        assertEquals(SCAN_CALL_COUNT, rowSource.readCount)
    }

    @Test
    fun `a content source failure fails the scan with that exception`() {
        val imageRow = imageRow()
        val videoRow = videoRow()
        val failure = IOException(CONTENT_FAILURE_MESSAGE)
        val content =
            FakeContentSource(
                contentByUri = mapOf(imageRow.uri to ABC_BYTES),
                failureByUri = mapOf(videoRow.uri to failure),
            )

        val thrown =
            assertThrows(IOException::class.java) {
                scanner(rows = FakeMediaRowSource(listOf(imageRow, videoRow)), content = content).scan()
            }

        assertSame(failure, thrown)
    }

    private fun scanner(
        rows: MediaRowSource,
        content: MediaContentSource,
        metadataTime: MediaMetadataTimeSource = RecordingMetadataTimeSource(),
        deviceZone: ZoneId = BERLIN_ZONE,
    ): MediaStoreVersionScanner =
        MediaStoreVersionScanner(
            rows = rows,
            content = content,
            metadataTime = metadataTime,
            deviceZone = deviceZone,
        )

    private fun imageRow(dateModifiedEpochSeconds: Long = MediaTestData.DATE_MODIFIED_EPOCH_SECONDS): RawMediaEntry =
        MediaTestData.row(
            id = IMAGE_ID,
            mimeType = MediaTestData.IMAGE_MIME_TYPE,
            dateModifiedEpochSeconds = dateModifiedEpochSeconds,
        )

    private fun videoRow(): RawMediaEntry = MediaTestData.row(id = VIDEO_ID, mimeType = MediaTestData.VIDEO_MIME_TYPE)

    private class FakeContentSource(
        private val contentByUri: Map<String, ByteArray> = emptyMap(),
        private val failureByUri: Map<String, IOException> = emptyMap(),
    ) : MediaContentSource {
        val opened = mutableListOf<CloseTrackingInputStream>()

        override fun open(uri: String): InputStream {
            val failure = failureByUri[uri]
            if (failure != null) {
                throw failure
            }
            return CloseTrackingInputStream(contentByUri.getValue(uri)).also { opened += it }
        }
    }

    private class CloseTrackingInputStream(
        bytes: ByteArray,
    ) : ByteArrayInputStream(bytes) {
        var isClosed = false
            private set

        override fun close() {
            isClosed = true
            super.close()
        }
    }

    private class RecordingMetadataTimeSource(
        private val captureTimeByUri: Map<String, MediaCaptureTime> = emptyMap(),
    ) : MediaMetadataTimeSource {
        val calls = mutableListOf<MetadataCall>()

        override fun captureTime(
            uri: String,
            mimeType: String,
        ): MediaCaptureTime? {
            calls += MetadataCall(uri = uri, mimeType = mimeType)
            return captureTimeByUri[uri]
        }
    }

    private data class MetadataCall(
        val uri: String,
        val mimeType: String,
    )

    private companion object {
        const val IMAGE_ID = 1L
        const val AUDIO_ID = 2L
        const val VIDEO_ID = 3L

        const val MODIFIED_EPOCH_SECONDS = 1_700_000_123L
        const val MODIFIED_EPOCH_MILLIS = 1_700_000_123_000L

        const val ABC_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        const val EMPTY_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

        const val IMAGE_MIME_PREFIX = "image/"
        const val VIDEO_MIME_PREFIX = "video/"

        const val CONTENT_FAILURE_MESSAGE = "cannot read original bytes"
        const val SCAN_CALL_COUNT = 2

        const val FIXED_METADATA_OFFSET_HOURS = 2
        const val FIXED_METADATA_EPOCH_MILLIS = 1_672_531_200_000L
        const val BERLIN_SUMMER_EPOCH_MILLIS = 1_689_319_541_000L

        val ABC_BYTES = "abc".toByteArray(Charsets.UTF_8)
        val EMPTY_BYTES = ByteArray(0)
        val BERLIN_ZONE: ZoneId = ZoneId.of("Europe/Berlin")
        val FIXED_METADATA_TIME =
            MediaCaptureTime(
                dateTime = LocalDateTime.of(2023, 1, 1, 2, 0),
                offset = ZoneOffset.ofHours(FIXED_METADATA_OFFSET_HOURS),
            )
        val ZONE_LESS_METADATA_TIME =
            MediaCaptureTime(
                dateTime = LocalDateTime.of(2023, 7, 14, 9, 25, 41),
                offset = null,
            )
    }
}
