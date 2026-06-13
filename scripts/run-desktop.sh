#!/usr/bin/env bash
# Сборка и запуск десктоп-приложения (Slint, отображается через WSLg).
# Запуск из WSL: bash scripts/run-desktop.sh
# Для второй копии локально: P2P_DATA_DIR=/tmp/u2 bash scripts/run-desktop.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"
export DISPLAY="${DISPLAY:-:0}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"

bash scripts/build-ygg.sh >/dev/null
cargo run -p p2p-desktop
