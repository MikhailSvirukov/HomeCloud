# Test environment

Reproducible environment for exercising the Android client, the dedicated
server, and the multi-node storage cluster described in
[`docs/specification-v1.md`](../docs/specification-v1.md) (sections 5, 8, 9, 11)
and [`docs/agent-test-environment.md`](../docs/agent-test-environment.md).

This first iteration is a **bootstrap skeleton**: the Rust workspace, a
Docker-independent cluster model with in-memory fakes, and an extensible
fault/recovery scenario engine. Docker-based scenarios (Testcontainers) are
follow-up work.

## Layout

```
test-environment/
├── Cargo.toml             # workspace (resolver 3, edition 2024)
├── scripts/ci.sh          # area-local quality gates
└── crates/
    ├── harness-core/      # Docker-independent model + fakes
    │   └── src/{topology,fault,node,error}.rs
    └── harness-scenario/  # scenario definition + execution engine
        └── src/scenario.rs
```

- `harness-core::topology`: fixed cluster — one dedicated server plus a
  parameterised, odd number of storage nodes with stable ids, labels, and
  addresses; enforces quorum arithmetic (`n / 2 + 1`).
- `harness-core::fault`: the two base fault classes (full node shutdown and
  selective peer-link degradation with delay/loss) behind a `FaultInjector`
  trait and an in-memory `FakeFaultInjector`.
- `harness-core::node`: `NodeControl` lifecycle (start/stop/restart) with an
  `InMemoryNodeControl` fake.
- `harness-scenario`: `Scenario`/`ScenarioBuilder` (ordered `Action`s), the
  `ScenarioCheck` extension point for new recovery assertions, and
  `ScenarioRunner`, which returns a per-step `ScenarioReport`.

## Gates

```sh
test-environment/scripts/ci.sh
```

Fast isolated tests use the in-memory fakes and need no Docker; Docker
integration scenarios are intentionally separate and are not run by `ci.sh`.

## Notes

- The crates may only build on published public traits/types (or Kotlin
  protocol/API interfaces) once those are published under `contracts/`; they
  must not depend on another component's private implementation.
- `Cargo.lock` is committed so harness dependency resolution is reproducible.
