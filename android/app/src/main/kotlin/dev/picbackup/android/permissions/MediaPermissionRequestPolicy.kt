package dev.picbackup.android.permissions

/**
 * Outcome of the media-read permission decision for the caller.
 */
enum class MediaPermissionStatus {
    /** Every required media-read permission is granted, so the caller has nothing left to ask for. */
    GRANTED,

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
object MediaPermissionRequestPolicy {

    /**
     * The required media-read permissions the caller still has to request, in the order the media
     * permission policy lists them. It is empty when everything required is granted and never
     * contains permissions the policy does not require.
     */
    fun pendingPermissions(sdkInt: Int, granted: Set<String>): List<String> =
        MediaPermissionPolicy.requiredPermissions(sdkInt).filterNot(granted::contains)

    /**
     * Decides how the caller should continue. [MediaPermissionStatus.GRANTED] means nothing is
     * missing and no prompt is needed; [MediaPermissionStatus.REQUESTABLE] means the caller may ask
     * for the pending permissions once; [MediaPermissionStatus.BLOCKED] means a still-missing
     * required permission was permanently refused, so the caller must send the user to the system
     * settings instead of asking again. A permanently denied permission that is already granted does
     * not block.
     */
    fun status(
        sdkInt: Int,
        granted: Set<String>,
        permanentlyDenied: Set<String>,
    ): MediaPermissionStatus {
        val pending = pendingPermissions(sdkInt, granted)
        return when {
            pending.isEmpty() -> MediaPermissionStatus.GRANTED
            pending.any(permanentlyDenied::contains) -> MediaPermissionStatus.BLOCKED
            else -> MediaPermissionStatus.REQUESTABLE
        }
    }
}
