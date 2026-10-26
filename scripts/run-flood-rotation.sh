#!/usr/bin/env bash
# Эксперимент к ADR-007 (пункт 0): останавливает ли ротация ключа поток флудера?
# Топология A → R → B (без прямого линка), замер байт на линках до и после смены
# ключа жертвой. Запуск из WSL/Linux: bash scripts/run-flood-rotation.sh [аргументы]
#
# Полезные варианты:
#   --secs N --pps N --size N   параметры потока (по умолчанию 12с, 400 пкт/с, 1 КБ)
#   --direct                    контроль: прямой линк A↔B (маршрутизации нет)
#   --no-rejoin                 жертва после ротации не возвращается в сеть
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh
cargo run -p peerage-ygg --example flood_rotation -- "$@"
