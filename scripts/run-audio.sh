#!/usr/bin/env bash
# Юнит-тесты медиа (кодек/джиттер) + сквозной аудио-loopback (синтетика).
# Запуск из WSL: bash scripts/run-audio.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
echo "=== юнит-тесты peerage-media ==="
cargo test -q -p peerage-media 2>&1 | grep -E "test result|error" | head
echo "=== аудио loopback ==="
cargo run -q -p peerage-media --example audio_loopback
