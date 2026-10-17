#!/usr/bin/env bash
# Area-local quality gates for the test-environment Rust workspace.
#
# Runs the gates required by the repository-root AGENTS.md. Daemon-free tests
# run first; ignored Docker-backed integration tests run in a second pass and
# require a Docker daemon in the CI environment.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_root="$(cd "${script_dir}/.." && pwd)"
cd "${workspace_root}"

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --workspace --all-features --locked -- --ignored
