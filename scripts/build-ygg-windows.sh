#!/usr/bin/env bash
# Собирает libygg.dll (+ libygg.h и импорт-библиотеку) под Windows x86_64 — та же
# c-shared обёртка вокруг yggdrasil-go, что libygg.so, кросс-компиляцией из WSL/Linux.
# Нужен mingw-w64: sudo apt install gcc-mingw-w64-x86-64
# Запуск: bash scripts/build-ygg-windows.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$PATH"
export CGO_ENABLED=1 GOOS=windows GOARCH=amd64 CC=x86_64-w64-mingw32-gcc
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/ygg/libygg/windows-amd64"
mkdir -p "$OUT"
cd "$ROOT/ygg"
go build -trimpath -ldflags "-s -w" -buildmode=c-shared -o "$OUT/libygg.dll" ./libygg
# Импорт-библиотека для линковки из MSVC/Rust (*-pc-windows-msvc): .def из таблицы
# экспортов DLL, .lib — dlltool.
{
  echo "LIBRARY libygg.dll"
  echo "EXPORTS"
  # Флаг, а не exit: ранний выход awk рвал бы objdump (SIGPIPE) и с pipefail — сборку.
  x86_64-w64-mingw32-objdump -p "$OUT/libygg.dll" |
    awk '/Ordinal\/Name Pointer\] Table/ {f = 1; next} f && /^$/ {f = 0} f && /^[[:space:]]*\[/ {print $NF}'
} > "$OUT/libygg.def"
x86_64-w64-mingw32-dlltool -d "$OUT/libygg.def" -D libygg.dll -l "$OUT/libygg.lib"
echo "built: $OUT"
ls -la "$OUT"
