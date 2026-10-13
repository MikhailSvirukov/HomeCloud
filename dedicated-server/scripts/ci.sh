#!/usr/bin/env bash
# Area-local CI for the dedicated-server workstream.
set -euo pipefail

area_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$area_root"
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features

echo "dedicated-server: all checks passed"
