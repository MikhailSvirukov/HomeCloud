package dev.homecloud.android.notifications

/**
 * Decision helper for the runtime notification permission (`FR-AND-035`). The app asks for it on a
 * fresh launch before showing background-upload notifications. The request starts at API 33
 * because the platform requires the permission there, which covers the Android 15 wording of
 * `FR-AND-035`. A refusal never blocks media scanning or backup (`FR-AND-036`): the scheduler and
 * the upload worker do not consult this policy, only notification posting does.
 */
internal object NotificationPermissionPolicy {
    /** Runtime notification permission on API 33 and above. */
    const val POST_NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"

    /** First platform level that requires the runtime notification permission. */
    const val NOTIFICATION_PERMISSION_SDK_INT = 33

    /**
     * Reports whether the app must ask for the notification permission: the platform requires it
     * and the permission is not granted.
     */
    fun shouldRequest(
        sdkInt: Int,
        granted: Boolean,
    ): Boolean = sdkInt >= NOTIFICATION_PERMISSION_SDK_INT && !granted
}
