#!/usr/bin/env bash
# Проверка лёгкого медиа-пути (демукс чат+медиа) на localhost.
# Запуск из WSL: bash scripts/run-media.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
cargo run -q -p peerage-messenger --example media_loopback
