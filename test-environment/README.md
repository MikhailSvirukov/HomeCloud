# Test environment

Reproducible environment for exercising the Android client, the dedicated
server, and the multi-node storage cluster described in
[`docs/specification-v1.md`](../docs/specification-v1.md) (sections 5, 8, 9, 11)
and [`docs/agent-test-environment.md`](../docs/agent-test-environment.md).

A Rust harness for the fixed cluster: a Docker-independent model with in-memory
fakes, a Docker cluster plan with CLI rendering and runtimes, a scenario engine
with reusable checks and a scenario catalogue, and a shared fixture crate used by
the tests.

## Layout

```
test-environment/
├── Cargo.toml                  # workspace (resolver 3, edition 2024)
├── Cargo.lock
├── rust-toolchain.toml         # pinned toolchain (rustfmt, clippy)
├── AGENTS.md                   # area conventions and quality gates
├── scripts/ci.sh               # area-local quality gates
└── crates/
    ├── harness-core/           # Docker-independent model + fakes
    │   └── src/{lib,topology,fault,node,error}.rs
    ├── harness-docker/         # cluster plan, CLI rendering, runtimes, Docker faults
    │   └── src/{lib,plan,command,runtime,cli,process,bollard_runtime,docker_faults}.rs
    ├── harness-scenario/       # scenario engine, checks, catalogue, registry
    │   └── src/{lib,scenario,checks,catalogue,spec}.rs
    └── harness-test-support/   # fixtures shared by tests
        └── src/lib.rs
```

- `harness-core::topology`: fixed cluster — one dedicated server plus a
  parameterised, odd number of storage nodes with stable ids, labels, and
  addresses; enforces quorum arithmetic (`n / 2 + 1`).
- `harness-core::fault`: the two base fault classes (full node shutdown and
  selective peer-link degradation with delay/loss) behind a `FaultInjector`
  trait and an in-memory `FakeFaultInjector`.
- `harness-core::node`: `NodeControl` lifecycle (start/stop/restart) with an
  `InMemoryNodeControl` fake.
- `harness-docker::plan`: a pure `ClusterPlan` derived from a topology — the
  isolated network plus one container per node (image, hostname, address, env).
  The container environment is supplied by the caller: `PlanSettings::env` is an
  `EnvBuilder` function that builds the entries from each node spec, rather than
  a fixed set of keys. `PlanSettings::with_command` and
  `PlanSettings::with_cap_add` attach a per-container command and granted Linux
  capabilities to the plan.
- `harness-docker::command`: deterministic rendering of the Docker CLI commands
  that stand the plan up (`network create`, `run`, `stop`, `rm`), assertable
  without a daemon.
- `harness-docker::runtime`: the `ClusterRuntime` start/stop/remove seam with a
  `FakeClusterRuntime`, so lifecycle logic is testable without Docker.
- `harness-docker::cli`: `CliClusterRuntime`, a `ClusterRuntime` that drives the
  cluster through the Docker CLI over a `CommandRunner`; `RecordingCommandRunner`
  records the rendered commands instead of executing them, so lifecycle paths
  stay daemon-free.
- `harness-docker::process`: `ProcessCommandRunner`, the `CommandRunner` that
  executes the rendered commands as child processes.
- `harness-docker::bollard_runtime`: `BollardClusterRuntime`, the library-backed
  Docker API runtime required by TST-004; it drives the same `ClusterRuntime`
  lifecycle through `bollard`, pulling images that are not present locally.
- `harness-docker::docker_faults`: `DockerFaultInjector`, a `FaultInjector` that
  applies per-pair packet filtering inside the node containers through the
  Docker API (TST-006), with full drop and probabilistic loss.
- `harness-scenario::spec`: `ScenarioSpec` and an ordered `ScenarioRegistry` for
  registering and building scenarios without changing the runner (TST-009).
- `harness-scenario`: `Scenario`/`ScenarioBuilder` (ordered `Action`s), the
  `ScenarioCheck` extension point for new recovery assertions, `ScenarioRunner`
  (per-step `ScenarioReport`, stops at first failure), a library of reusable
  checks (`checks`: all nodes running, node running/stopped, storage quorum
  held/lost, fault active, no active faults), and a standard scenario catalogue
  (`catalogue`: node-shutdown, link-degradation, quorum-loss, and
  network-partition recovery) so fault/recovery scenarios are composed from
  shared blocks.
- `harness-test-support`: fixtures shared by tests — a fixture topology and the
  `nodes`/`faults` helpers bound to it, plus the fixture address used by node ids.

## Gates

GitHub Actions runs `scripts/ci.sh` for pull requests via
[`.github/workflows/test-environment.yml`](../.github/workflows/test-environment.yml).
OpenCode agents add or update tests but do not execute test suites locally;
use the PR checks as the result. Docker-backed integration tests are marked
`#[ignore]` and run in CI's ignored pass
(`cargo test --workspace --all-features --locked -- --ignored`) in the CI
environment that provides Docker.

Fast isolated tests use the in-memory fakes, the shared fixtures, and
`CliClusterRuntime` with `RecordingCommandRunner`, and need no Docker. The CLI
runtime can also be driven by `ProcessCommandRunner`, which runs real child
processes; Docker-backed end-to-end scenarios remain separate and run only in
the ignored pass.

## Notes

- The crates may only build on published public traits/types (or Kotlin
  protocol/API interfaces) once those are published under `contracts/`; they
  must not depend on another component's private implementation.
- `Cargo.lock` is committed so harness dependency resolution is reproducible.
