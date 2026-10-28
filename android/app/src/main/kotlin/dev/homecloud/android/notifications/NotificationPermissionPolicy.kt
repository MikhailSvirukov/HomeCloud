package dev.homecloud.android.notifications

/**
 * Decision helper for the runtime notification permission (`FR-AND-035`). The app asks for it
 * before showing background-upload notifications. A refusal never blocks media scanning or backup
 * (`FR-AND-036`): the scheduler and the upload worker do not consult this policy, only notification
 * posting does.
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

    /**
     * Reports whether a notification can be posted: the permission is granted, or the platform does
     * not require it yet. A false value must never block scanning or backup (`FR-AND-036`).
     */
    fun canShow(
        sdkInt: Int,
        granted: Boolean,
    ): Boolean = !shouldRequest(sdkInt, granted)
}
