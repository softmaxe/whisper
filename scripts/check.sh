#!/bin/bash
# Formatting, lint and tests: the checks CI runs on every push and pull request.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
