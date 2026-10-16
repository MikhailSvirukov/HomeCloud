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

## Code style

- Keep the public surface minimal: private modules re-exported with `pub use` from the crate root, fields internal unless a consumer needs them.
- Prefer public fields over accessors that only return a field; keep an accessor only when it guards an invariant.
- Derive only the traits that are used, and never derive `Debug` for a type that renders a collection or secret material.
- Model errors with `thiserror`, one variant per rule, and carry a small enum when rules differ only by field or role.
- Use `NonZero*` types instead of runtime zero checks.
- Use a module-local result alias, chain with `and_then`, and return early instead of `else if`.
- Prefer `match` over nested conditionals.
- Keep trait bounds and generics to the minimum.
- Put long inputs (TOML documents, templates, keys) in files rather than inline.
- Document intent only; for this area that narrows the root documentation rule: no `# Errors`/`# Returns` sections and no argument-by-argument narration.
- Write scenario-based tests: one case per scenario, no duplicate scenarios across unit and integration layers, and no tests for constructors or static data with no behavioural logic.
- Shipped code logs with `tracing` and does not print with `println!`/`eprintln!`.

## Quality gates

- `bash scripts/ci.sh` (fmt, clippy with `-D warnings`, tests) must pass before
  merge.
- No `unwrap`/`expect`/`panic!` outside tests; keep I/O at system boundaries.

## Reviews and pull requests

- Write issues and pull requests in English.
- Describe only what the change does: no verification or test-status sections,
  no scope section, no notes about work deliberately left out, and no line
  references into documents.
- Keep the description proportional to the change and stay within the area.
- End the pull request description with the issue reference (`Closes #N`).
- Write the review on the pull request itself as a `COMMENT` review carrying
  `[agent-review: changes-requested]` or `[agent-review: approved]`; the author
  and reviewer share one account, so GitHub does not accept an approval from the
  author.
