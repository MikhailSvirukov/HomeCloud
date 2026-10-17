package dev.homecloud.android.permissions

import dev.homecloud.android.R

/**
 * The single action the media-read permission surface offers together with its message.
 */
internal enum class MediaPermissionAction {
    /** Ask the user for the still-missing media-read permissions. */
    REQUEST,

    /** Send the user to the app's system settings to re-enable the refused permissions. */
    OPEN_SETTINGS,

    /** Offer no action, because access is already granted. */
    NONE,
}

/**
 * The message and action the media-read permission surface shows for a permission state. The
 * message id is one of the `media_permission_*` string resources.
 */
internal data class MediaPermissionPrompt(
    val messageResId: Int,
    val action: MediaPermissionAction,
)

/**
 * Decides what the media-read permission surface shows for the current API level, granted and
 * refused permissions and whether a request has already been answered. The mapping is delegated to
 * [MediaPermissionRequestPolicy], so a full grant wins, selected access offers another request,
 * a pending refusal blocks, and a still-missing permission that was not refused is requested.
 *
 * The refused set only influences the outcome once a request has been answered. Before that Android
 * cannot tell a refusal from a permission that was never asked for, so the policy is asked with an
 * empty refused set while no answer exists. That is why the answered flag is part of the decision.
 */
internal fun mediaPermissionPrompt(
    sdkInt: Int,
    granted: Set<String>,
    permanentlyDenied: Set<String>,
    hasRequested: Boolean,
): MediaPermissionPrompt {
    val refused = if (hasRequested) permanentlyDenied else emptySet()
    return when (MediaPermissionRequestPolicy.status(sdkInt, granted, refused)) {
        MediaPermissionStatus.GRANTED ->
            MediaPermissionPrompt(
                messageResId = R.string.media_permission_granted_message,
                action = MediaPermissionAction.NONE,
            )

        MediaPermissionStatus.LIMITED ->
            MediaPermissionPrompt(
                messageResId = R.string.media_permission_limited_message,
                action = MediaPermissionAction.REQUEST,
            )

        MediaPermissionStatus.REQUESTABLE ->
            MediaPermissionPrompt(
                messageResId = R.string.media_permission_request_message,
                action = MediaPermissionAction.REQUEST,
            )

        MediaPermissionStatus.BLOCKED ->
            MediaPermissionPrompt(
                messageResId = R.string.media_permission_blocked_message,
                action = MediaPermissionAction.OPEN_SETTINGS,
            )
    }
}
