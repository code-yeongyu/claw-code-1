#!/usr/bin/env sh
set -eu

usage() {
  cat <<'EOF'
Usage: ./install.sh [--system] [--cargo-bin] [--bin-dir <dir>]

Builds the release `claw` binary and installs it into one of:
  ~/.cargo/bin   (default)
  /usr/local/bin (with --system)
  <dir>          (with --bin-dir)
EOF
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR=${CLAW_INSTALL_DIR:-}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --system)
      BIN_DIR=/usr/local/bin
      shift
      ;;
    --cargo-bin)
      BIN_DIR=${HOME}/.cargo/bin
      shift
      ;;
    --bin-dir)
      if [ "$#" -lt 2 ]; then
        usage >&2
        exit 1
      fi
      BIN_DIR=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
done

if [ -z "$BIN_DIR" ]; then
  BIN_DIR=${HOME}/.cargo/bin
fi

mkdir -p "$BIN_DIR"

cargo build --release --manifest-path "$SCRIPT_DIR/rust/Cargo.toml" -p rusty-claude-cli
install -m 755 "$SCRIPT_DIR/rust/target/release/claw" "$BIN_DIR/claw"

printf 'Installed claw to %s\n' "$BIN_DIR/claw"

case ":${PATH:-}:" in
  *":$BIN_DIR:"*)
    ;;
  *)
    printf 'warning: add %s to PATH if `claw` is not yet discoverable\n' "$BIN_DIR" >&2
    ;;
esac
