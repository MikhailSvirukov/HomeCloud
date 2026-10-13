# Android client

Kotlin/Jetpack Compose Android client for the HomeCloud v1 photo/video backup system. The app finds photos and videos available to it, compresses and encrypts them, and uploads them to the server cluster in the background. This directory is an independent Gradle project rooted at `android/`; it is not part of any Rust workspace.

## v1 scope

- Automatic backup of existing media at first run and of new/changed media afterwards.
- Request only the photo/video read permissions needed for backup and background transfer.
- Stable `upload_id` per file version across retries and node switches.
- Queue and retry uploads when the cluster is unreachable.

Non-goals (not in v1): download/view/restore, client-side version management, chunk/offset resume, propagating local deletes to the server, automatic key rotation, and the dedicated-server admin panel.

## Implemented so far

- Photo/video classification from raw media rows (`media/MediaCatalog`).
- The media read permission policy that selects permissions per API level (`permissions/MediaPermissionPolicy`).
- An in-memory upload queue holding one entry per stable `upload_id`, with `requeueFailed()` for the next retry cycle (`queue/InMemoryUploadQueue`).
- Not yet implemented: persistence, background scheduling (WorkManager), MediaStore access, and networking/protocol.

## Declared permissions

The manifest declares exactly three permissions: `READ_MEDIA_IMAGES`, `READ_MEDIA_VIDEO`, and `READ_EXTERNAL_STORAGE` capped with `android:maxSdkVersion="32"`. On API 33 and above the client uses the two per-type reads; below 33 it uses the legacy read, matching `MediaPermissionPolicy.requiredPermissions(sdkInt)`. No audio, contacts, location, camera, or storage-write permission is declared.

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

- Root project name is `PicBackup`; the application module is `:app`.
- Namespace and applicationId are `dev.picbackup.android`.
- Kotlin test sources live under `app/src/test/kotlin` in package `dev.picbackup.android`.
- Test framework is JUnit 4. `buildConfig` is enabled so `dev.picbackup.android.BuildConfig` is available to JVM unit tests.
- Planned background mechanism is Android WorkManager, to be introduced by a later issue, not in this scaffold.

## Directory layout

```
android/
├── AGENTS.md              area-local agent instructions
├── README.md              this file
├── ci/
│   └── check.sh           area-local CI: bootstrap Gradle and run build + unit tests
├── settings.gradle.kts    Gradle settings and module includes
├── build.gradle.kts       root build configuration
├── gradle.properties      shared Gradle properties
└── app/
    ├── build.gradle.kts   application module configuration
    └── src/
        ├── main/kotlin/dev/picbackup/android/
        │   ├── MainActivity.kt   Compose entry point
        │   ├── media/            photo/video classification from raw media rows
        │   ├── permissions/      which media read permissions to request
        │   └── queue/            in-memory upload queue and retry bookkeeping
        └── test/kotlin/dev/picbackup/android/
            ├── BuildWiringTest.kt   build wiring checks
            ├── media/               unit tests for media classification
            ├── permissions/         unit tests for the permission policy
            └── queue/               unit tests for the upload queue
```

## Build and test

No Gradle wrapper JAR is committed: the repository publishing path is text-only, so committing a binary wrapper artifact is not possible. Instead, `ci/check.sh` downloads the pinned `gradle-8.14.3-bin` distribution on first use, verifies its SHA-256 checksum, unzips it into a cache, and runs the build.

Run it from any working directory (invoke it through `bash` so it does not depend on the stored executable bit):

```sh
bash android/ci/check.sh
```

The script resolves the `android/` directory from its own location and executes:

```sh
"$GRADLE_BIN" --no-daemon --stacktrace -p "$ANDROID_DIR" :app:assembleDebug :app:testDebugUnitTest
```

Before invoking Gradle it exports `GRADLE_USER_HOME="${GRADLE_USER_HOME:-$HOME/.gradle}"` and `ANDROID_USER_HOME="${ANDROID_USER_HOME:-$HOME/.android}"`. The JVM's `user.home` can resolve to a read-only path in agent containers (for example `/home/node`) while `$HOME` is writable, so Gradle's native services and the Android tools need writable home directories. Both variables honor caller-provided overrides.

The distribution is cached at `${GRADLE_DIST_CACHE:-$HOME/.cache/picbackup-gradle}`; set `GRADLE_DIST_CACHE` to relocate the cache. The script requires only `bash`, `curl`, `sha256sum`, and `unzip`, and fails with a non-zero exit code if any step fails.
