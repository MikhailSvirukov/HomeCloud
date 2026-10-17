package dev.homecloud.android.permissions

/**
 * Outcome of the media-read permission decision for the caller.
 */
internal enum class MediaPermissionStatus {
    /** Every required media-read permission is granted, so the caller has nothing left to ask for. */
    GRANTED,

    /**
     * The user allowed access only to selected photos and videos. This is usable access rather than
     * a refusal, and the caller may ask again so the user can change the selection.
     */
    LIMITED,

    /** Some required permissions are still missing but may be asked for once. */
    REQUESTABLE,

    /**
     * At least one still-missing required permission was permanently refused, so the caller must
     * send the user to the system settings instead of asking again.
     */
    BLOCKED,
}

/**
 * Decision helper for the Android runtime media-read permission flow. It requests nothing itself:
 * it only reports which permissions still have to be asked for and whether asking is still
 * possible or the user has permanently refused.
 */
internal object MediaPermissionRequestPolicy {
    /**
     * The required media-read permissions the caller still has to request, in the order the media
     * permission policy lists them. It is empty when everything required is granted and never
     * contains permissions the policy does not require.
     */
    fun pendingPermissions(
        sdkInt: Int,
        granted: Set<String>,
    ): List<String> = MediaPermissionPolicy.requiredPermissions(sdkInt).filterNot(granted::contains)

    /**
     * Reports the permission status for the given API level, granted set and permanently refused
     * set, in that precedence order: nothing pending wins, then selected access on API 34 and above,
     * then a refused pending permission, then the ordinary request case.
     */
    fun status(
        sdkInt: Int,
        granted: Set<String>,
        permanentlyDenied: Set<String>,
    ): MediaPermissionStatus {
        val pending = pendingPermissions(sdkInt, granted)
        return when {
            pending.isEmpty() -> MediaPermissionStatus.GRANTED
            sdkInt >= MediaPermissionPolicy.VISUAL_USER_SELECTED_SDK_INT &&
                granted.contains(MediaPermissionPolicy.READ_MEDIA_VISUAL_USER_SELECTED) ->
                MediaPermissionStatus.LIMITED
            pending.any(permanentlyDenied::contains) -> MediaPermissionStatus.BLOCKED
            else -> MediaPermissionStatus.REQUESTABLE
        }
    }
}
