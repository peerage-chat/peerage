#!/usr/bin/env bash
# Собирает libygg.so и запускает сквозной тест чата (E2E текст через Yggdrasil).
# Запуск из WSL/Linux: bash scripts/run-chat.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# rpath из build.rs transport не доходит до бинарей других крейтов — указываем явно.
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
cargo run -p p2p-messenger --example chat_loopback
