#!/usr/bin/env bash
# Доказательства Kani (план беты, шаг 6). Запуск из WSL:
#   scripts/kani.sh              — все доказательства p2p-core
#   scripts/kani.sh unpad        — только те, чьё имя содержит подстроку
#
# Фаззер проверяет выборку входов, Kani — ВСЕ входы ограниченного размера
# (bounded model checking). Доказательства живут рядом с кодом в `mod proofs`
# под cfg(kani) и в обычную сборку не попадают.
# Установка один раз: cargo install --locked kani-verifier && cargo kani setup
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f scripts/local.env ] && . scripts/local.env
# Свой каталог сборки: Kani собирает своим тулчейном и флагами.
TD="${CARGO_TARGET_DIR:-$ROOT/target}-kani"
if [ $# -gt 0 ]; then
    exec cargo kani -p p2p-core --target-dir "$TD" --harness "$1"
fi
exec cargo kani -p p2p-core --target-dir "$TD"
