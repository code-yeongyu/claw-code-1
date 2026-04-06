#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"
TESTS_DIR="$REPO_ROOT/tests"

cd "$REPO_ROOT"

echo "=== claw-code verify ==="
echo "repo: $REPO_ROOT"

if [ ! -f "$RUST_DIR/Cargo.toml" ]; then
    echo "FAIL: rust workspace not found at $RUST_DIR/Cargo.toml" >&2
    echo "=== verify FAILED ===" >&2
    exit 1
fi

if [ ! -d "$TESTS_DIR" ]; then
    echo "WARN: tests/ directory is missing at repo root"
fi

echo "--- cargo test --workspace ---"
if (cd "$RUST_DIR" && cargo test --workspace); then
    echo "=== verify PASSED ==="
    exit 0
fi

status=$?
echo "=== verify FAILED ===" >&2
exit "$status"
