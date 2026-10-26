#!/usr/bin/env bash
# Headless-проверка группового звонка (full mesh, видео по источникам).
# Запуск из WSL: bash scripts/run-group-call.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"
bash scripts/build-ygg.sh >/dev/null
cargo run -q -p peerage-call --example group_call_loopback
