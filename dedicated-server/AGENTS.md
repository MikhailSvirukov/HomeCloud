# Dedicated-server area guidance

Read and follow the repository-root `AGENTS.md` (Rust practices) and
[`docs/agent-global.md`](../docs/agent-global.md) before changing anything here.

## Scope

This area owns the Rust dedicated-server control-plane service: client
registration, client topology responses, fixed-cluster bootstrap information,
and its independent node heartbeat. It is not a file proxy and not a Raft
transport, and it must never accept, store, or proxy photo/video data.

Do not read or modify other workstream areas (`android/`,
`distributed-server/`, `test-environment/`). Coordinate shared
protocol/contract changes through `contracts/` and an issue.

## Quality gates

- `bash scripts/ci.sh` (fmt, clippy with `-D warnings`, tests) must pass before
  merge.
- No `unwrap`/`expect`/`panic!` outside tests; keep I/O at system boundaries.
