#!/usr/bin/env bash
# Сканирование зависимостей на уязвимости и проблемы лицензий.
# Требует cargo-deny: cargo install cargo-deny
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v cargo-deny >/dev/null 2>&1; then
    echo "cargo-deny не установлен. Установка: cargo install cargo-deny" >&2
    exit 1
fi

cargo deny check advisories bans sources licenses
