# AGENTS — test environment

This area follows the repository-root [`AGENTS.md`](../AGENTS.md) and the global
agent rules in [`docs/agent-global.md`](../docs/agent-global.md). Work only
inside `test-environment/`, plus published public traits/types or Kotlin
protocol/API interfaces required to build mocks and fakes.

## Scope

- Rust test harness, in-memory mocks/fakes, and Docker-based multi-container
  integration scenarios for the v1 components.
- Keep orchestration and test logic in Rust. Do not add unrelated Python or
  shell test frameworks; `scripts/` holds only thin gate wrappers.

## Rules

- Never inspect private implementation details of another workstream. Build on
  published contracts under `contracts/` or public traits/types only; if a
  contract is missing, file an issue for the owning workstream.
- Keep fast isolated tests (in-memory fakes, no Docker) separate from Docker
  integration scenarios. `scripts/ci.sh` runs only the fast gates.
- Prefer the two base fault classes from the brief — node shutdown and selective
  network degradation — and keep the scenario API extensible for recovery checks.
- Follow the root Rust practices: typed errors with `thiserror`, no `unwrap`/
  `expect` outside tests, one behaviour per test with a descriptive name.

## Quality gates

Run `test-environment/scripts/ci.sh` before hand-off. It runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```
