#!/usr/bin/env bash
set -euo pipefail

# Entry point for the distributed-server quality gate.
# Runs from the workspace root so it can be called from any directory.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace_root="$(cd -- "${script_dir}/.." && pwd)"
cd -- "${workspace_root}"

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features
