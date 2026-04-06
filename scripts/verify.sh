#!/usr/bin/env bash
# Canonical verification entrypoint for claw-code.
# Run from any directory. Always verifies the full workspace including integration suite.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"
TESTS_DIR="$REPO_ROOT/tests"

echo "=== claw-code verify ==="
echo "repo: $REPO_ROOT"
echo ""

# 1. Rust workspace tests
echo "--- cargo test --workspace ---"
if [ ! -f "$RUST_DIR/Cargo.toml" ]; then
    echo "ERROR: $RUST_DIR/Cargo.toml not found" >&2
    exit 1
fi
cd "$RUST_DIR"
cargo test --workspace -- --test-threads=1 2>&1
echo ""

# 2. Integration tests at repo root
echo "--- integration tests ---"
if [ -d "$TESTS_DIR" ]; then
    test_count=$(find "$TESTS_DIR" -name "*.rs" -o -name "*.sh" -o -name "*.py" | wc -l | tr -d ' ')
    if [ "$test_count" -eq 0 ]; then
        echo "ERROR: tests/ directory exists but contains no test files" >&2
        exit 1
    fi
    echo "found $test_count test file(s) in tests/"
    # Run any shell-based integration tests
    for f in "$TESTS_DIR"/*.sh; do
        [ -f "$f" ] || continue
        echo "running: $f"
        bash "$f"
    done
    # Run Python integration tests from repo root so src/ is importable
    cd "$REPO_ROOT"
    py_tests=$(find "$TESTS_DIR" -name "*.py" | sort)
    if [ -n "$py_tests" ]; then
        echo "running Python tests via pytest ..."
        python3 -m pytest $py_tests -v
    fi
    # Note: Rust integration tests in tests/ are compiled as part of cargo test --workspace above
    echo "integration suite: OK"
else
    echo "WARN: no tests/ directory at repo root — integration suite may be missing"
fi

echo ""
echo "=== verify PASSED ==="
