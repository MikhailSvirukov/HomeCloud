# Android workstream — area-local agent instructions

Read the root `/workspace/AGENTS.md` first for repository-wide process rules; its Rust language practices do not apply to Kotlin code here. Then read [README.md](README.md) and the shared v1 specification `/workspace/docs/specification-v1.md`.

## Scope

- This area is the Kotlin/Jetpack Compose Android client.
- v1 scope is one-directional photo/video backup only: discover media, compress, encrypt, upload, and retry. No download, viewing, restore, chunk resume, client-side version management, server-side delete, or automatic key rotation.
- Do not implement anything outside that scope. Server-side and test-harness behavior belongs to other workstreams.

## Read scope

- Do not browse, search, list, summarize, or adapt to source in `distributed-server/`, `dedicated-server/`, or `test-environment/`.
- Use only the published v1 specification `/workspace/docs/specification-v1.md` and published interfaces under `/workspace/contracts/` for cross-component behavior. Do not invent wire formats locally; request a contract change through the normal issue/PR flow.
- Work only inside `android/`.

## Android-specific rules

- Request only photo/video read permissions needed for backup and background transfer. Never request audio, contacts, location, or unrelated permissions.
- Keep a stable `upload_id` per file version across retries and across storage-node switches.
- Do not propagate local file deletion to the server. A deleted local file does not delete the server copy.
- Never log plaintext passwords, filenames, or payload contents. Do not place them in network logs, crash reports, or analytics.
- Use vetted cryptographic libraries and standard schemes. Never implement custom cryptographic primitives.
- The upload/queue domain model now exists; the background scheduling mechanism (Android WorkManager) is still to be introduced by a later issue.

## Code and review style

- Documentation describes behavior only; it never lists arguments, return values, or errors.
- Tests cover distinct scenarios and must not verify static data or trivial constructors; one scenario is not tested twice.
- Pull-request and issue text is English only and describes what the change contains, with the issue-closing keyword on the last line.
- Use coroutines where the code needs asynchronous or background work; do not introduce manual threading or blocking calls on the main thread in their place.
- Reviewing a change covers style, structure, clarity, and duplication, not only formatting and lint output.

## Test and review conventions

- Android framework code is tested in the unit-test source set (`app/src/test/kotlin`) with Robolectric; do not add a new test dependency without pinning it in the version catalog `gradle/libs.versions.toml`.
- The module build file (`app/build.gradle.kts`) points the forked unit-test JVM's `user.home` at the Gradle user home, so Robolectric's runtime jars land in the directory the CI workflow caches.
- A framework test that cannot run on a `noexec` temp filesystem disables Conscrypt with `@ConscryptMode(ConscryptMode.Mode.OFF)`, matching the area's other Robolectric tests.
- A change is reviewed on its pull request with the `[agent-review: ...]` marker, not only in a local report.

## Quality gate

Run the area-local check before handing off:

```sh
bash android/ci/check.sh
```

It bootstraps the pinned Gradle distribution and runs `:app:assembleDebug` and `:app:testDebugUnitTest`.

## Pinned tool versions

compileSdk 36, targetSdk 36, minSdk 26, build-tools 36.0.0, Java/JVM 17, AGP 8.13.2, Gradle 8.14.3, Kotlin 2.4.21, Compose BOM 2026.06.01 (compose-ui 1.11.4 / material3 1.4.0), core-ktx 1.18.0, activity-compose 1.13.0, lifecycle-runtime-ktx 2.10.0, JUnit 4.13.2.
