package dev.homecloud.android.media

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import dev.homecloud.android.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.coroutines.cancellation.CancellationException

/**
 * Shows how much media the client can see. It reads the library once when it enters composition,
 * off the main thread, and reports the found counts, a loading line while the read runs, or a
 * failure line when the library cannot be read.
 */
@Composable
internal fun MediaLibrarySummaryContent(
    discovery: MediaDiscovery,
    modifier: Modifier = Modifier,
) {
    val state by produceState<MediaLibrarySummaryState>(
        initialValue = MediaLibrarySummaryState.Loading,
        key1 = discovery,
    ) {
        value = loadLibrarySummary(discovery)
    }

    Text(
        text =
            when (val current = state) {
                MediaLibrarySummaryState.Loading -> stringResource(R.string.media_library_loading)
                is MediaLibrarySummaryState.Loaded ->
                    pluralStringResource(
                        R.plurals.media_library_found,
                        current.summary.totalCount,
                        current.summary.totalCount,
                    )

                MediaLibrarySummaryState.Failed -> stringResource(R.string.media_library_failed)
            },
        modifier = modifier,
        style = MaterialTheme.typography.bodyLarge,
    )
}

/** The three outcomes of reading the library summary. */
private sealed interface MediaLibrarySummaryState {
    /** The library read is still running. */
    data object Loading : MediaLibrarySummaryState

    /** The library read finished with the [summary] it found. */
    data class Loaded(
        val summary: MediaLibrarySummary,
    ) : MediaLibrarySummaryState

    /** The library read failed. */
    data object Failed : MediaLibrarySummaryState
}

/** Reads and summarizes the library on the IO dispatcher without letting a failure escape. */
private suspend fun loadLibrarySummary(discovery: MediaDiscovery): MediaLibrarySummaryState =
    try {
        val summary = withContext(Dispatchers.IO) { summarize(discovery.discoverBackupCandidates()) }
        MediaLibrarySummaryState.Loaded(summary)
    } catch (cancellation: CancellationException) {
        throw cancellation
    } catch (failure: Exception) {
        MediaLibrarySummaryState.Failed
    }
