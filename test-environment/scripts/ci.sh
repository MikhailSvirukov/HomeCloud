#!/usr/bin/env bash
# Area-local quality gates for the test-environment Rust workspace.
#
# Runs the gates required by the repository-root AGENTS.md. Integration tests
# that need Docker are not started here; they run separately through the
# Testcontainers-based scenarios.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_root="$(cd "${script_dir}/.." && pwd)"
cd "${workspace_root}"

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
