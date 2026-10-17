package dev.homecloud.android.permissions

/**
 * Decision helper for the Android runtime media-read permissions this client may request on a given
 * API level. It requests nothing itself and never returns audio, location, contacts, camera, or
 * storage-write permissions.
 */
internal object MediaPermissionPolicy {
    /** Per-type photo read permission on API 33 and above. */
    const val READ_MEDIA_IMAGES = "android.permission.READ_MEDIA_IMAGES"

    /** Per-type video read permission on API 33 and above. */
    const val READ_MEDIA_VIDEO = "android.permission.READ_MEDIA_VIDEO"

    /** Broad media read permission below API 33. */
    const val READ_EXTERNAL_STORAGE = "android.permission.READ_EXTERNAL_STORAGE"

    /** Permission for the user-selected subset of photos and videos on API 34 and above. */
    const val READ_MEDIA_VISUAL_USER_SELECTED = "android.permission.READ_MEDIA_VISUAL_USER_SELECTED"

    /** First API level at which media reads are split into per-type permissions. */
    const val PER_TYPE_MEDIA_PERMISSIONS_SDK_INT = 33

    /** First API level at which the user may allow access to a selected subset of photos and videos. */
    const val VISUAL_USER_SELECTED_SDK_INT = 34

    private val perTypeReadPermissions = listOf(READ_MEDIA_IMAGES, READ_MEDIA_VIDEO)
    private val legacyReadPermissions = listOf(READ_EXTERNAL_STORAGE)

    /**
     * Decides the permissions needed for full access to photos and videos at a given API level: the
     * per-type permissions from [PER_TYPE_MEDIA_PERMISSIONS_SDK_INT] upward, the legacy one below.
     */
    fun requiredPermissions(sdkInt: Int): List<String> =
        if (sdkInt >= PER_TYPE_MEDIA_PERMISSIONS_SDK_INT) perTypeReadPermissions else legacyReadPermissions

    /**
     * Decides the permissions passed to one runtime request at a given API level. From
     * [VISUAL_USER_SELECTED_SDK_INT] upward the request also carries
     * [READ_MEDIA_VISUAL_USER_SELECTED], so a single system prompt covers full, user-selected and
     * denied media access; below that level it matches [requiredPermissions].
     */
    fun requestedPermissions(sdkInt: Int): List<String> =
        if (sdkInt >= VISUAL_USER_SELECTED_SDK_INT) {
            perTypeReadPermissions + READ_MEDIA_VISUAL_USER_SELECTED
        } else {
            requiredPermissions(sdkInt)
        }
}
