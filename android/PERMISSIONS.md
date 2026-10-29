# Android permission inventory

This inventory is the FR-AND-050 record of every permission the app declares in its manifest,
every platform permission a dependency merges into that manifest, and every runtime request. Each
row names the v1 requirement that justifies the permission and the feature it serves. A declared
permission without such a link must be removed.

| Permission | Requirement(s) | Feature | Requested at runtime |
| --- | --- | --- | --- |
| `android.permission.READ_MEDIA_IMAGES` | FR-AND-037, FR-AND-038, FR-AND-049 | Reading photos for backup on API 33 and above | Yes, on API 33 and above |
| `android.permission.READ_MEDIA_VIDEO` | FR-AND-037, FR-AND-038, FR-AND-049 | Reading videos for backup on API 33 and above | Yes, on API 33 and above |
| `android.permission.READ_EXTERNAL_STORAGE` | FR-AND-004, FR-AND-032 | Reading photos and videos for backup below API 33 | Yes, below API 33 |
| `android.permission.READ_MEDIA_VISUAL_USER_SELECTED` | FR-AND-037, FR-AND-039, FR-AND-049 | Backing up the photos and videos the user selected on API 34 and above | Yes, on API 34 and above |
| `android.permission.INTERNET` | FR-AND-051 | TCP connection to the dedicated server and storage nodes | No, granted at install time |
| `android.permission.POST_NOTIFICATIONS` | FR-AND-035, FR-AND-036 | Showing the ongoing foreground-transfer notification; a refusal only skips that notification and never stops scanning or backup | Yes, on API 33 and above |
| `android.permission.FOREGROUND_SERVICE_DATA_SYNC` | CON-AND-005 | Lets the upload transfer service run as a `dataSync` foreground service for the duration of a long upload | No |
| `android.permission.WAKE_LOCK` | CON-AND-001, FR-AND-032 | Lets WorkManager keep the device awake while a backup cycle runs; merged by `androidx.work`, not declared by this app | No |
| `android.permission.ACCESS_NETWORK_STATE` | CON-AND-001 | Lets WorkManager observe connectivity for its own scheduling bookkeeping; merged by `androidx.work`, not declared by this app | No |
| `android.permission.RECEIVE_BOOT_COMPLETED` | CON-AND-001 | Lets WorkManager restore persisted cycles after a device reboot; merged by `androidx.work`, not declared by this app | No |
| `android.permission.FOREGROUND_SERVICE` | CON-AND-004, CON-AND-005 | Lets `UploadTransferService` show a long upload and lets WorkManager run foreground work; declared by this app, and the `androidx.work` merge carries the same entry | No |

The merged manifest also contains
`dev.homecloud.android.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION`, which this app does not declare.
`androidx.core` injects it as a signature-level permission for non-exported dynamic receiver
registration. It is install-time only, is never requested at runtime, and serves no v1 product
feature; it is recorded here for completeness against the merged manifest.
