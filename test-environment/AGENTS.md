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
  integration scenarios. CI runs the applicable suites; agent containers must
  not run tests locally because the test agent has no local Docker daemon.
- Prefer the two base fault classes from the brief — node shutdown and selective
  network degradation — and keep the scenario API extensible for recovery checks.
- Follow the root Rust practices: typed errors with `thiserror`, no `unwrap`/
  `expect` outside tests, one behaviour per test with a descriptive name.

## Rust review conventions

These area conventions complement the repository-root [`AGENTS.md`](../AGENTS.md);
that document remains the baseline, and this section records the review points
that recur most in `test-environment/`.

- **Types over strings.** Keep numeric data in a numeric type instead of `String`,
  and type the domain (bounded newtypes, `NonZero*`) so range/zero checks
  disappear. Model limited sets as enums and compare with the type's own traits
  (`PartialEq`/`Ord`) rather than reading inner fields.
- **Errors.** Use distinct `thiserror` variants for distinct failures; do not
  funnel every case into a generic `Invalid` carrying a free-form message. When a
  module has one error type, define `type Result<T>` there. A value reused by
  several checks belongs in a constructor or check function, not many file-level
  constants.
- **API surface.** Keep public fields and functions minimal and curate the
  exported surface with `pub use`. Do not replace a public field with a getter
  method.
- **Control flow.** Prefer `match` arms over `if`/`else` inside a `match`, avoid
  `else if` in favour of early returns, and use `ToPrimitive`/`FromPrimitive`
  instead of large enum-to-number `match`es. Prefer `and_then` for chains that
  only propagate with `?`.
- **Docs.** Do not add `# Errors`/`# Returns` sections or per-argument
  descriptions; describe failure modes on the error variants instead.
- **Tests.** Cover distinct scenarios only — never test the same scenario twice —
  and do not test trivial constructors, getters, or static data. Form an expected
  command/result in a dedicated helper.
- No `unwrap`/`expect`/`panic!` outside tests (see Rules above). This area does
  not need logging, so do not add direct printing.

## Quality gates

GitHub Actions runs `test-environment/scripts/ci.sh` through
[`.github/workflows/test-environment.yml`](../.github/workflows/test-environment.yml).
Do not run the script from an agent container. Add or update the tests and have
the orchestrator verify the required Actions result on the PR before merge. The
CI script runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --workspace --all-features --locked -- --ignored
```

The ignored-tests pass runs the Docker-backed integration tests in the CI
environment that provides Docker.
