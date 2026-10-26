#!/usr/bin/env bash
# Воспроизводимая сборка APK в Docker (план беты, шаг 7). Запуск из WSL:
#   scripts/repro-build.sh [каталог_вывода=repro-out] [ref=HEAD]
#   scripts/repro-build.sh --check   — собрать ДВАЖДЫ в чистых контейнерах и сравнить
#
# Собирается закоммиченное состояние (git clone внутри контейнера), результат —
# неподписанный APK и .so с SHA256SUMS. Подпись ставится отдельно; сверка подписанного
# релиза с этой сборкой — reproducible/README.md.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE=peerage-repro:1

docker build -q -t "$IMAGE" "$ROOT/reproducible" >/dev/null

run() { # $1 — каталог вывода, $2 — ref
    rm -rf "$1" && mkdir -p "$1"
    docker run --rm -v "$ROOT:/src:ro" -v "$(realpath "$1"):/out" -e REF="$2" "$IMAGE"
}

if [ "${1:-}" = "--check" ]; then
    ref="${2:-HEAD}"
    a="$(mktemp -d)" b="$(mktemp -d)"
    run "$a" "$ref"
    run "$b" "$ref"
    if diff "$a/SHA256SUMS" "$b/SHA256SUMS"; then
        echo "ВОСПРОИЗВОДИМО: две чистые сборки совпали побайтно."
    else
        echo "РАСХОЖДЕНИЕ: сборки различаются (каталоги $a и $b)." >&2
        exit 1
    fi
    exit 0
fi

run "${1:-$ROOT/repro-out}" "${2:-HEAD}"
