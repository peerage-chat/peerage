#!/usr/bin/env bash
# Собирает libygg.so и запускает loopback-пример (настоящий Yggdrasil через FFI).
# Запуск из WSL/Linux: bash scripts/run-loopback.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl   # отдельно от Windows-сборки
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh
cargo run -p p2p-ygg --example loopback
