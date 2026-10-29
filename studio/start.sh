#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
for command in cargo trunk; do
  if ! command -v "$command" >/dev/null; then
    echo "Falta ${command}. Instalá Rust y Trunk antes de iniciar." >&2
    exit 1
  fi
done
cargo run --manifest-path backend/Cargo.toml &
bridge_pid=$!
cleanup() { kill "$bridge_pid" 2>/dev/null || true; wait "$bridge_pid" 2>/dev/null || true; }
trap cleanup EXIT INT TERM
trunk serve --address 127.0.0.1 --port 3060
