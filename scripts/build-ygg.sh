#!/usr/bin/env bash
# Собирает libygg.so (c-shared обёртку вокруг yggdrasil-go).
# Запуск из WSL/Linux: bash scripts/build-ygg.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$PATH"
export CGO_ENABLED=1
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/ygg"
go mod tidy >/dev/null 2>&1
go build -buildmode=c-shared -o libygg/libygg.so ./libygg
echo "built: $ROOT/ygg/libygg/libygg.so"
ls -la libygg/
