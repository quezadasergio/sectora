#!/usr/bin/env bash
# Local/CI checks before tagging a release.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Sectora release validation"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "ERROR: missing command: $1"
    exit 1
  }
}

need node
need npm
need cargo
need rustc

echo "OK  toolchains present"
echo "    rustc $(rustc --version)"
echo "    node  $(node --version)"

test -f package.json
test -f src-tauri/tauri.conf.json
test -f src-tauri/Cargo.toml
test -f .github/workflows/release.yml
test -f THIRD_PARTY_DDRESCUE.md
test -f assets/app-icon.png
echo "OK  required project files"

# Sidecar for current host (best-effort; REQUIRE in CI)
if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
  BIN=src-tauri/binaries/ddrescue-aarch64-apple-darwin
  if [[ ! -x "$BIN" ]]; then
    echo "Preparing ddrescue sidecar…"
    REQUIRE_DDRESCUE=1 bash scripts/fetch-ddrescue.sh
  fi
  "$BIN" --version | head -1
  echo "OK  macOS Apple Silicon ddrescue sidecar"
elif [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]]; then
  BIN=src-tauri/binaries/ddrescue-x86_64-unknown-linux-gnu
  if [[ ! -x "$BIN" ]] || ! file "$BIN" | grep -qi ELF; then
    REQUIRE_DDRESCUE=1 bash scripts/fetch-ddrescue.sh
  fi
  "$BIN" --version | head -1
  echo "OK  Linux ddrescue sidecar"
else
  echo "WARN host $(uname -s)/$(uname -m): skip native sidecar check"
fi

# Version consistency
NODE_V=$(node -p "require('./package.json').version")
TAURI_V=$(node -p "require('./src-tauri/tauri.conf.json').version")
CARGO_V=$(sed -n 's/^version = \"\(.*\)\"/\1/p' src-tauri/Cargo.toml | head -1)
if [[ "$NODE_V" != "$TAURI_V" || "$NODE_V" != "$CARGO_V" ]]; then
  echo "ERROR version mismatch: package=$NODE_V tauri=$TAURI_V cargo=$CARGO_V"
  exit 1
fi
echo "OK  versions aligned at $NODE_V"

echo
echo "Validation passed. To cut a release candidate:"
echo "  git tag v${NODE_V}-rc.1 && git push origin v${NODE_V}-rc.1"
echo "Or production:"
echo "  git tag v${NODE_V} && git push origin v${NODE_V}"
