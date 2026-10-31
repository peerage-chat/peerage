#!/usr/bin/env bash
# Мутационное тестирование (cargo-mutants): портит код по одному месту и смотрит,
# заметят ли тесты. Выживший мутант — непроверенное поведение. Из WSL:
#   scripts/mutants.sh -p litertc                      — весь крейт
#   scripts/mutants.sh -p litertc -f litertc/src/probe.rs   — один файл
#   scripts/mutants.sh -p litertc --list               — только перечислить мутантов
#   scripts/mutants.sh -p litertc --since HEAD~1       — только строки, изменённые с
#                                                         коммита (и незакоммиченные)
# Аргументы уходят в cargo mutants; отчёт — в $CARGO_TARGET_DIR/mutants/mutants.out
# (missed.txt — выжившие, caught.txt — пойманные); MUTANTS_OUT — другой каталог, чтобы
# гонять второй прогон рядом с идущим.
#
# Принятые равносильные мутанты (разобраны, безвредны) и исключённые файлы — в
# .cargo/mutants.toml: cargo-mutants читает его сам, повторно их не гоняет.
#
# Тесты в один поток и libygg в LD_LIBRARY_PATH — как в test-all.sh. Каждая копия
# дерева собирается в своём target: общий CARGO_TARGET_DIR копии заперли бы друг друга.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f scripts/local.env ] && . scripts/local.env
OUT="${MUTANTS_OUT:-${CARGO_TARGET_DIR:-target}/mutants}"
unset CARGO_TARGET_DIR
export PATH="$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

mkdir -p "$OUT"
# --since <коммит> (в любом месте) → --in-diff по `git diff <коммит>`: проверять
# тронутые строки, а не весь крейт (полный прогон litertc — около часа).
args=()
while [ $# -gt 0 ]; do
    if [ "$1" = "--since" ]; then
        git diff "$2" -- '*.rs' > "$OUT/since.diff"
        args+=(--in-diff "$OUT/since.diff")
        shift 2
    else
        args+=("$1")
        shift
    fi
done
set -- "${args[@]}"
exec cargo mutants --test-workspace=false -j "${JOBS:-4}" -o "$OUT" "$@" \
    -- --lib --tests -- --test-threads=1
