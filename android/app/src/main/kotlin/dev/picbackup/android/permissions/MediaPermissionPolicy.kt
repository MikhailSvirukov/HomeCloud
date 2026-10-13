package dev.picbackup.android.permissions

/**
 * Declares which Android runtime permissions this client may request for media
 * backup, and nothing else.
 *
 * The policy only decides which photo/video read permissions are needed for a
 * given API level. It does not request them: no runtime permission request UI is
 * implemented here, and the manifest must declare exactly these permissions in a
 * later issue. Audio, location, contacts, camera, and storage-write permissions
 * are never returned.
 */
object MediaPermissionPolicy {

    /** `READ_MEDIA_IMAGES`: per-type photo read permission on API 33 and above. */
    const val READ_MEDIA_IMAGES = "android.permission.READ_MEDIA_IMAGES"

    /** `READ_MEDIA_VIDEO`: per-type video read permission on API 33 and above. */
    const val READ_MEDIA_VIDEO = "android.permission.READ_MEDIA_VIDEO"

    /** `READ_EXTERNAL_STORAGE`: broad media read permission below API 33. */
    const val READ_EXTERNAL_STORAGE = "android.permission.READ_EXTERNAL_STORAGE"

    /** First API level at which media reads are split into per-type permissions. */
    const val PER_TYPE_MEDIA_PERMISSIONS_SDK_INT = 33

    private val perTypeReadPermissions = listOf(READ_MEDIA_IMAGES, READ_MEDIA_VIDEO)
    private val legacyReadPermissions = listOf(READ_EXTERNAL_STORAGE)

    /**
     * Returns the runtime permissions required to read photos and videos on [sdkInt].
     *
     * On API [PER_TYPE_MEDIA_PERMISSIONS_SDK_INT] (33) and above the per-type
     * [READ_MEDIA_IMAGES] and [READ_MEDIA_VIDEO] permissions are returned. Below
     * that boundary the legacy [READ_EXTERNAL_STORAGE] permission is returned.
     * The result never contains audio, location, contacts, camera, or
     * storage-write permissions. The returned list is immutable.
     *
     * @param sdkInt the device's `Build.VERSION.SDK_INT`.
     * @return the permissions this client may request, in a stable order.
     */
    fun requiredPermissions(sdkInt: Int): List<String> =
        if (sdkInt >= PER_TYPE_MEDIA_PERMISSIONS_SDK_INT) perTypeReadPermissions else legacyReadPermissions
}
