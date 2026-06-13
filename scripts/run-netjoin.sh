#!/usr/bin/env bash
# Проба входа в глобальную сеть Yggdrasil через публичные пиры.
# Запуск из WSL: bash scripts/run-netjoin.sh   (YGG_DEBUG=1 для логов линков)
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
cargo run -q -p p2p-ygg --example netjoin -- peers/public-peers.txt
