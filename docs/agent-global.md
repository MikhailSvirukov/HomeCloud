# Global agent instructions

These instructions apply to all four workstreams and are copied with the public monorepo into each workstream clone. The shared project boundary consists of this file, `docs/specification-v1.md`, the assigned workstream brief and role file, explicitly published API/protocol contracts under `contracts/`, root `AGENTS.md` for Rust practices, and daily review files mounted read-only. Other workstream briefs and role files are not shared context.

## Workstream ownership and read scope

Each workstream has its own clone of the same monorepo and its own checkout, branch, OpenCode state, and task history. Use the same configured Git author/committer identity in all four clones. Do not use another workstream's clone or checkout.

The workstream map is:

- `android/`: Android client and Android tests.
- `distributed-server/`: distributed Rust storage nodes and their component tests.
- `dedicated-server/`: Rust registration/bootstrap/control-plane service and its component tests.
- `test-environment/`: Rust test harness, Docker scenarios, shared test utilities, mocks, and fakes.

The assigned orchestrator owns only its mapped area. It may delegate only tasks inside that area to its own worker subagents, and those workers may read and change only files in their assigned task paths. The reviewer receives the same scope and may inspect the complete diff for that workstream only. The orchestrator must define explicit file/path ownership before delegating and must not ask a worker or reviewer to inspect another workstream's implementation.

### Empty-repository bootstrap ownership

The repository may begin empty. During an explicitly assigned bootstrap issue, an orchestrator may create its own mapped top-level directory (`android/`, `distributed-server/`, `dedicated-server/`, or `test-environment/`) and add that area’s manifests, local `AGENTS.md`, starter documentation, and area-local CI scripts/configuration. Keep the Android Gradle project and each Rust Cargo workspace rooted independently in their mapped directories; do not combine them into a root workspace. The test-environment bootstrap uses only the v1 specification and published contracts to establish mocks/fakes and a harness skeleton.

Root-level shared files (including root `AGENTS.md`, README, common configuration, and `.github/workflows/`) require an issue that explicitly names the shared paths and assigns one workstream to own that change. GitHub Actions workflow YAML belongs under `.github/workflows/`; workflows there should call the area-local CI checks rather than relocating component configuration into another area. This is a narrow permission for those files only; it does not grant permission to browse or modify another workstream’s source. Do not have multiple workstreams initialize the same shared files concurrently.

Do not browse, search, list, summarize, or adapt to source files owned by another workstream. A need to change a shared contract or cross-workstream interface is raised as an issue/PR for the owning workstream; do not inspect its private implementation to solve the integration. Treat root build files, dependency manifests, and shared code as shared scope: read or change them only when the task requires it, explain the dependency to the operator, and keep changes narrowly scoped.

The test-environment workstream may inspect only public Rust traits/types and public Kotlin API/protocol interfaces needed to build mocks, fakes, or integration tests. It must not inspect private component implementations. If an interface is not published as a stable contract, ask its owning workstream to publish the contract.

The orchestrator enforces these boundaries in task selection, decomposition, file ownership, and review. If a task cannot be completed within its workstream boundary, stop at a safe checkpoint and report the missing shared contract or ownership decision. Do not widen scope by silently reading another area.

These are agent workflow rules, not an operating-system security boundary. The clones contain the public monorepo and the containers have outbound network access; a determined process could retrieve other public files. Do not claim filesystem-level isolation unless the container mounts and repository layout actually enforce it.
