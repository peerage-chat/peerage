#!/usr/bin/env bash
# CRAP по функциям крейта: покрытие тестами + цикломатическая сложность. Из WSL:
#   scripts/crap.sh -p litertc          — функции с CRAP > 10
#   scripts/crap.sh -p litertc --lib    — только юнит-тесты (аргументы уходят в cargo)
#   LIMIT=5 scripts/crap.sh -p litertc  — свой порог
#
# Нужны cargo-llvm-cov (+ rustup component llvm-tools-preview) и rust-code-analysis-cli.
# Окружение — как в test-all.sh: libygg в LD_LIBRARY_PATH и тесты в один поток.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f scripts/local.env ] && . scripts/local.env
T="${CARGO_TARGET_DIR:-target}"
export PATH="$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

[ $# -gt 0 ] || { echo "укажи крейт: scripts/crap.sh -p <крейт>" >&2; exit 2; }
LCOV="$T/crap.lcov"
cargo llvm-cov --lcov --output-path "$LCOV" "$@" -- --test-threads=1 >&2
python3 scripts/crap.py "$LCOV" "$ROOT" "${LIMIT:-10}"
