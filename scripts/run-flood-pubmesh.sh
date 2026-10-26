#!/usr/bin/env bash
# Прогон к ADR-007 на НАСТОЯЩЕМ публичном меше Yggdrasil: доходит ли флуд на старый
# ключ до узла, поднявшего новый ключ? Наши A и B входят через публичные пиры, без
# прямого линка. Темп щадящий (чужая волонтёрская инфраструктура) — не поднимать зря.
# Запуск из WSL/Linux: bash scripts/run-flood-pubmesh.sh [аргументы]
#
#   --secs N --pps N --size N   параметры потока (по умолчанию 10с, 50 пкт/с, 400 Б)
#   --split                     A и B на непересекающихся половинах списка пиров
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh
cargo run -p peerage-ygg --example flood_pubmesh -- peers/public-peers.txt "$@"
