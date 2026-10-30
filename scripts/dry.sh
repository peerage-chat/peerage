#!/usr/bin/env bash
# Повторяющийся код (jscpd 5) в Rust и Kotlin: точные копии и функции, похожие по
# структуре на 0,8 и выше (--similarity). Из Git Bash (jscpd — npm-пакет Windows):
#   scripts/dry.sh                    — весь репозиторий
#   scripts/dry.sh litertc engine     — только эти каталоги
#   MIN_TOKENS=40 scripts/dry.sh      — ловить и короткие повторы (по умолчанию 70)
#
# Тесты не в счёт: повторяющаяся подготовка в них — норма, а не долг.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ $# -gt 0 ] || set -- .
exec jscpd \
    --format "rust,kotlin" \
    --min-tokens "${MIN_TOKENS:-70}" \
    --similarity "${SIMILARITY:-0.8}" \
    --ignore "**/tests/**,**/test/**,**/target/**,**/build/**,**/examples/**,**/fuzz/**,desktop/**,docs/**" \
    --reporters console \
    "$@"
