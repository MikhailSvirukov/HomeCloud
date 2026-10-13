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
- The planned background mechanism is Android WorkManager, introduced together with the upload/queue work; it is not part of the scaffold.

## Quality gate

Run the area-local check before handing off:

```sh
bash android/ci/check.sh
```

It bootstraps the pinned Gradle distribution and runs `:app:assembleDebug` and `:app:testDebugUnitTest`.

## Pinned tool versions

compileSdk 36, targetSdk 36, minSdk 26, build-tools 36.0.0, Java/JVM 17, AGP 8.13.2, Gradle 8.14.3, Kotlin 2.4.21, Compose BOM 2026.06.01 (compose-ui 1.11.4 / material3 1.4.0), core-ktx 1.18.0, activity-compose 1.13.0, lifecycle-runtime-ktx 2.10.0, JUnit 4.13.2.
