#!/usr/bin/env bash
# Прогнать все фаззинг-цели (план беты, шаг 2). Запуск из WSL:
#   scripts/fuzz.sh [секунд_на_цель=60] [цель…]
# Находки — в fuzz/artifacts/<цель>/; воспроизвести: cargo +nightly fuzz run <цель> <файл>.
# Корпус копится в fuzz/corpus/ (не в git): каждый следующий прогон продолжает с него.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/fuzz"
[ -f "$ROOT/scripts/local.env" ] && . "$ROOT/scripts/local.env"
# Сборка фаззера — отдельно от основной: другой тулчейн и флаги.
TD="${CARGO_TARGET_DIR:-$ROOT/target}-fuzz"
SECS="${1:-60}"
shift || true
TARGETS=("$@")
[ ${#TARGETS[@]} -eq 0 ] && TARGETS=($(cargo +nightly fuzz list))
LOG="$(mktemp)"
fail=0
for t in "${TARGETS[@]}"; do
    echo "== $t (${SECS}s) =="
    # rss_limit: телефон, а не сервер — гигабайт на один вход уже находка.
    cargo +nightly fuzz run --target-dir "$TD" "$t" -- \
        -max_total_time="$SECS" -rss_limit_mb=1024 -malloc_limit_mb=512 -print_final_stats=1 >"$LOG" 2>&1
    code=$?
    grep -E "^(SUMMARY|stat::number_of_executed_units|stat::peak_rss_mb)|panicked|out-of-memory|Test unit written" "$LOG"
    if [ $code -ne 0 ]; then
        fail=1
        echo "!! $t: найдена проблема (код $code, см. fuzz/artifacts/$t)"
    fi
done
rm -f "$LOG"
exit $fail
