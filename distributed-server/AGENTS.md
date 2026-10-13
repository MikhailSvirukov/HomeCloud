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

## Security

Key and credential material (private keys, signing keys, passwords, verifiers) must
never be logged or committed.
