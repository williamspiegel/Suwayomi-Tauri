#!/bin/bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TAURI_DIR="$ROOT_DIR/desktop/tauri/src-tauri"

cd "$ROOT_DIR"

./scripts/prepare-tauri-resources.sh

if ! cargo tauri --help >/dev/null 2>&1; then
  echo "cargo-tauri is not installed."
  echo "Install with: cargo install tauri-cli --version '^2.0'"
  exit 1
fi

cd "$TAURI_DIR"
cargo tauri build "$@"
