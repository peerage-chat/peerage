#!/usr/bin/env bash
# Полный прогон Rust-набора. Запускать из WSL:
#   scripts/test-all.sh            — весь набор
#   scripts/test-all.sh -p p2p-core --lib   — часть (аргументы уходят в cargo test)
#
# Три вещи, которые каждый раз приходится вспоминать заново, — поэтому они здесь:
#
#  1. --test-threads=1. Тесты движка ждут событий по РЕАЛЬНОМУ времени, а параллельный
#     прогон поднимает десяток движков со своими потоками на 6 ядрах: таймауты начинают
#     срабатывать раньше, чем приходит ожидаемое, и падения выглядят как «плавающие».
#  2. LD_LIBRARY_PATH до ygg/libygg. Транспорт линкуется с libygg.so; без пути
#     тесты не стартуют вовсе, с сообщением про отсутствующую библиотеку.
#  3. --exclude p2p-desktop и --lib --tests. Desktop-крейт и examples/call_signaling
#     не компилируются (унаследованное старое API звонков); без исключения упрёшься в
#     чужую ошибку компиляции, так и не запустив ни одного теста.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# Машинные настройки (напр. CARGO_TARGET_DIR на другом диске) — в scripts/local.env,
# он в .gitignore. Без него сборка идёт в ./target, как обычно.
[ -f scripts/local.env ] && . scripts/local.env
T="${CARGO_TARGET_DIR:-target}"

export PATH="$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

if [ $# -gt 0 ]; then
    exec cargo test "$@" -- --test-threads=1
fi

exec cargo test --workspace --exclude p2p-desktop --lib --tests -- --test-threads=1
