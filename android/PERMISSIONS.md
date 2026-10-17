# Android permission inventory

This inventory is the FR-AND-050 record of every permission declared in the merged manifest and
every runtime request. Each row names the v1 requirement that justifies the permission and the
feature it serves. A permission without such a link must be removed.

| Permission | Requirement(s) | Feature | Requested at runtime |
| --- | --- | --- | --- |
| `android.permission.READ_MEDIA_IMAGES` | FR-AND-037, FR-AND-038, FR-AND-049 | Reading photos for backup on API 33 and above | Yes, on API 33 and above |
| `android.permission.READ_MEDIA_VIDEO` | FR-AND-037, FR-AND-038, FR-AND-049 | Reading videos for backup on API 33 and above | Yes, on API 33 and above |
| `android.permission.READ_EXTERNAL_STORAGE` | FR-AND-004, FR-AND-032 | Reading photos and videos for backup below API 33 | Yes, below API 33 |
| `android.permission.READ_MEDIA_VISUAL_USER_SELECTED` | FR-AND-037, FR-AND-039, FR-AND-049 | Backing up the photos and videos the user selected on API 34 and above | Yes, on API 34 and above |
| `android.permission.INTERNET` | FR-AND-051 | TCP connection to the dedicated server and storage nodes | No, granted at install time |
