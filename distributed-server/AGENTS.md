# distributed-server

Area-local guidance for the distributed Rust storage nodes. The repository-root
[`../../AGENTS.md`](../../AGENTS.md) is the authoritative Rust practice guide; follow it
before editing any Rust here.

Shared context lives in the [v1 specification](../../docs/specification-v1.md) and the
[workstream brief](../../docs/agent-distributed-server.md). Acceptance criteria are
defined there, not in this file.

## Boundary

- Read and modify only files under `distributed-server/`.
- Do not inspect the Android, dedicated-server, or test-harness areas.
- Shared protocol/interface changes go through issues/PRs for the owning workstream;
  do not widen scope by reading another area's private implementation.

## Quality gate

`./ci/check.sh` is the required local gate. It runs, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
3. `cargo test --workspace --all-features`

## Code conventions

These follow the daily review notes and apply to every Rust change here.

- Keep the public surface minimal. Modules stay private and items are exposed through the
  curated `pub use` facade in `lib.rs`; a module (and its fields) becomes public only when
  its purpose requires it.
- Prefer public fields over one-line getters that only return a field.
- Keep trait bounds to the minimum the code needs.
- Avoid derives unless they are needed. Never derive `Debug` on a type that owns large
  buffers, pools, or collections; write a compact manual `Debug` that prints counts and
  sizes, never payload bytes.
- Model errors as distinct enum variants with `thiserror`, not a generic `Invalid`
  carrying a message. Do not add one variant per field; make invalid values
  unrepresentable with `NonZero*` types. A module with one error type gets a local
  `type Result<T> = core::result::Result<T, ErrorKind>;` alias.
- Prefer guard-style `if ... { return Err(..) }` groups and `match` over `else if` chains
  and nested `if`/`else` (this refines the root guide's general `if`/`else` preference).
- When a chain of fallible steps only propagates errors with `?`, express it as an
  `and_then` chain where that reads more clearly.
- Enums with stable wire values use explicit `#[repr(u8)]` discriminants or
  `From`/`TryFrom` conversions rather than parallel `match` tables.
- Document behaviour only. Do not add `# Errors`/`# Returns` sections or per-argument
  prose; describe what each error means on the error variant itself (this refines the
  root guide's documentation rule).
- Tests cover each scenario once: internal edge cases as unit tests, public behaviour in
  `tests/`. Do not test trivial constructors or static data, and do not repeat a scenario
  in both layers.
- Pass limits, sizes, keys, and addresses in explicitly; do not hide required inputs
  inside constants.
- Keep long static data (large certificates, key material, fixtures) in separate files
  read at runtime; do not embed it in code.

## Pull requests and issues

- Write issues and pull requests in English.
- Describe only what the change includes; do not list what is out of scope and do not
  reference line numbers.
- Do not add "Verification"/test-result sections.
- Put `Closes #N` as the last line of the pull request description.
- Review is about style and practices, not only formatting and lints.

## Security

Key and credential material (private keys, signing keys, passwords, verifiers) must
never be logged or committed.
