#!/usr/bin/env bash
# Сквозной тест группового чата (MLS, 3 узла) через реальный Yggdrasil, без сервера.
# Запуск из WSL: bash scripts/run-group.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"
bash scripts/build-ygg.sh >/dev/null
cargo run -q -p p2p-groupchat --example group_chat
