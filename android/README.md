# Android client

Kotlin/Jetpack Compose Android client for the HomeCloud v1 photo/video backup system. The app finds photos and videos available to it, compresses and encrypts them, and uploads them to the server cluster in the background. This directory is an independent Gradle project rooted at `android/`; it is not part of any Rust workspace.

## v1 scope

- Automatic backup of existing media at first run and of new/changed media afterwards.
- Request only the photo/video read permissions needed for backup and background transfer.
- Stable `upload_id` per file version across retries and node switches.
- Queue and retry uploads when the cluster is unreachable.

Non-goals (not in v1): download/view/restore, client-side version management, chunk/offset resume, propagating local deletes to the server, automatic key rotation, and the dedicated-server admin panel.

## Implemented so far

- Reading photo and video rows from `MediaStore` and turning the cursor columns into raw media entries (`media/MediaStoreMediaRowSource`, `media/MediaDiscovery`, `media/MediaRowMapping`).
- Classifying raw media rows into backup candidates, dropping everything that is not a photo or video (`media/MediaCatalog`).
- The per-API-level read permission policy that selects the permissions needed to read photos and videos (`permissions/MediaPermissionPolicy`).
- The permission request decision: which required permissions are still missing and whether asking for them is still possible (`permissions/MediaPermissionRequestPolicy`).
- The version identity the SRS defines - content hash, original name and tracked time - with SHA-256 over the original bytes and the contract’s random UUIDv4 upload id (`media/MediaContentHasher`, `media/MediaVersionIdentity`).
- Planning driven by that identity: candidates are scanned with their identity and only untracked versions are queued, each with a new upload id, while a tracked version keeps its id (`media/MediaVersionCandidate`, `media/MediaVersionScanner`, `backup/MediaBackupPlanner`).
- The in-memory upload queue with its retry bookkeeping (`queue/InMemoryUploadQueue`, `queue/UploadQueue`).
- The persisted queue snapshot that survives a restart (`queue/FileUploadQueueStore`, `queue/UploadQueueStore`).
- The runner that sequences restore → plan → persist for one cycle (`backup/BackupCycleRunner`).
- A structured cycle log that a backup cycle records its outcome through, carrying only the cycle counts, defaulting to a no-op and with an `android.util.Log` implementation (`log/BackupCycleLog`, which also declares the no-op default, and `log/AndroidBackupCycleLog`).
- The launched activity's media-read permission surface, which shows the current permission status, requests the media-read permissions in one runtime operation (including the partial-access permission from Android 14), reports limited coverage when the user allows only some photos and videos, and opens the application's settings page when access was permanently refused (`permissions/MediaPermissionRequestContent`, `permissions/MediaPermissionPrompt`).
- The media-library summary shown once access is granted, reporting the combined number of photos and videos available to back up, with a loading line while the library is read and a failure line when it cannot be read (`media/MediaLibrarySummaryContent`, `media/MediaLibrarySummary`).
- The scanner that turns discovered rows into version candidates: it keeps photos and videos, hashes each item's original bytes once, resolves its tracked capture time and builds the version identity (`media/MediaStoreVersionScanner`, `media/MediaContentSource`, `media/MediaMetadataTimeSource`).
- Platform adapters for the scanner seams: original bytes through the content resolver, Exif capture time with the media store taken time as fallback, and the media store taken-time column carried through the row model (`media/ContentResolverMediaContentSource`, `media/ExifMediaMetadataTimeSource`, `media/ContentResolverExifDateTimeReader`).
- The tracked capture time rule: Exif date and offset parsing, metadata over media store, zone-less times in the device zone, Unix milliseconds (`media/CaptureTimeResolver`).
- Zstandard compression of a version into a bounded payload buffer chain with the full compressed size known before the first frame and a validated 1..19 level (`payload/MediaCompressor`, `payload/CompressedPayload`).
- Queue capacity from configuration (1..1 000 000 versions, default 10 000): planning stops at the limit, keeps every accepted entry and reports the rejected count through the cycle log (`backup/MediaBackupPlanner`, `log/BackupCycleLog`).
- Restoring the queue requeues an upload that was in flight when the process stopped (`queue/InMemoryUploadQueue`).
- Background execution through WorkManager: a worker runs one cycle, a factory builds the real graph, and a scheduler enqueues an idempotent six-hour periodic cycle and an on-demand one without charging, battery or network constraints (`backup/BackupWorker`, `backup/BackupWorkerFactory`, `backup/BackupCycleComposition`, `backup/BackupWorkScheduler`, `HomeCloudApplication`).

Not yet implemented: networking/protocol client, the foreground `dataSync` transfer service and upload notifications, and the setup and main screens.

## Declared permissions

The manifest permission set and the requirement each permission serves are tracked in [PERMISSIONS.md](PERMISSIONS.md).

## Pinned decisions

| Item | Version |
| --- | --- |
| compileSdk | 36 |
| targetSdk | 36 |
| minSdk | 26 |
| build-tools | 36.0.0 |
| Java/JVM | 17 |
| Android Gradle Plugin | 8.13.2 |
| Gradle | 8.14.3 |
| Kotlin | 2.4.21 |
| Compose BOM | 2026.06.01 (compose-ui 1.11.4 / material3 1.4.0) |
| core-ktx | 1.18.0 |
| activity-compose | 1.13.0 |
| lifecycle-runtime-ktx | 2.10.0 |
| JUnit | 4.13.2 |
| ktlint Gradle plugin | 14.2.0 |

- Root project name is `HomeCloud`; the application module is `:app`.
- Namespace and applicationId are `dev.homecloud.android`.
- Kotlin test sources live under `app/src/test/kotlin` in package `dev.homecloud.android`.
- Test framework is JUnit 4.
- Everything in the module is `internal` except `MainActivity`, the launched activity; the module surface is not a published contract.
- Background cycles run through Android WorkManager; `HomeCloudApplication` supplies the worker factory and the manifest removes the default startup initializer so WorkManager initialises on demand.

## Directory layout

```
android/
├── AGENTS.md              area-local agent instructions
├── README.md              this file
├── ci/
│   └── check.sh           area-local CI: bootstrap Gradle and run debug + release builds, unit tests, lint and style
├── settings.gradle.kts    Gradle settings and module includes
├── build.gradle.kts       root build configuration
├── gradle.properties      shared Gradle properties
└── app/
    ├── build.gradle.kts   application module configuration
    └── src/
        ├── main/kotlin/dev/homecloud/android/
        │   ├── MainActivity.kt   Compose entry point
        │   ├── backup/           planning discovered media into the queue
        │   │   ├── MediaBackupPlanner.kt   enqueues untracked versions from a scanner
        │   │   └── BackupCycleRunner.kt    sequences restore → plan → persist for one cycle
        │   ├── log/              structured per-cycle logging
        │   │   ├── BackupCycleLog.kt         structured cycle log and its no-op default
        │   │   └── AndroidBackupCycleLog.kt  reports each cycle through android.util.Log
        │   ├── media/            reading and classifying MediaStore rows
        │   │   ├── MediaStoreMediaRowSource.kt    queries the device MediaStore
        │   │   ├── MediaDiscovery.kt              reads all backup candidates
        │   │   ├── MediaCatalog.kt                classifies rows into candidates
        │   │   ├── MediaLibrarySummary.kt         counts discovered photos and videos
        │   │   └── MediaLibrarySummaryContent.kt  shows the summary, loading and failure lines
        │   ├── permissions/      which media read permissions to request
        │   │   ├── MediaPermissionPolicy.kt            required permissions per API level
        │   │   ├── MediaPermissionRequestPolicy.kt     missing permissions and whether to ask
        │   │   ├── MediaPermissionPrompt.kt            message and action for a permission state
        │   │   └── MediaPermissionRequestContent.kt    permission surface that requests or opens settings
        │   └── queue/            upload queue and its persisted snapshot
        │       ├── UploadQueue.kt           queue model and retry bookkeeping
        │       ├── InMemoryUploadQueue.kt   in-memory implementation
        │       ├── UploadQueueStore.kt      snapshot persistence boundary
        │       └── FileUploadQueueStore.kt  snapshot file that survives a restart
        └── test/kotlin/dev/homecloud/android/
            ├── backup/              unit tests for planning and the cycle runner
            ├── log/                 unit tests for the structured cycle log
            ├── media/               unit tests for media discovery and classification
            ├── permissions/         unit tests for the permission policy pair
            ├── queue/               unit tests for the queue and its store
            └── testsupport/         shared test fixtures for the media and backup unit tests
```

## Build and test

No Gradle wrapper JAR is committed: the repository publishing path is text-only, so committing a binary wrapper artifact is not possible. Instead, `ci/check.sh` downloads the pinned `gradle-8.14.3-bin` distribution on first use, verifies its SHA-256 checksum, unzips it into a cache, and runs the build.

GitHub Actions runs this check for pull requests. OpenCode agents add or update
tests but do not execute test suites locally; use the PR checks as the result.

Run it from any working directory (invoke it through `bash` so it does not depend on the stored executable bit):

```sh
bash android/ci/check.sh
```

The script resolves the `android/` directory from its own location and executes:

```sh
"$GRADLE_BIN" --no-daemon --stacktrace -p "$ANDROID_DIR" :app:assembleDebug :app:assembleRelease :app:testDebugUnitTest :app:lintDebug :app:ktlintCheck
```

Before invoking Gradle it exports `GRADLE_USER_HOME="${GRADLE_USER_HOME:-$HOME/.gradle}"` and `ANDROID_USER_HOME="${ANDROID_USER_HOME:-$HOME/.android}"`. The JVM's `user.home` can resolve to a read-only path in agent containers (for example `/home/node`) while `$HOME` is writable, so Gradle's native services and the Android tools need writable home directories. Both variables honor caller-provided overrides.

The distribution is cached at `${GRADLE_DIST_CACHE:-$HOME/.cache/homecloud-gradle}`; set `GRADLE_DIST_CACHE` to relocate the cache. The script requires only `bash`, `curl`, `sha256sum`, and `unzip`, and fails with a non-zero exit code if any step fails.
