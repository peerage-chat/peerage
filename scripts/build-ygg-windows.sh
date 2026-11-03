#!/usr/bin/env bash
# Собирает под Windows x86_64 кросс-компиляцией из WSL/Linux:
# - libygg.dll (+ libygg.h и импорт-библиотеку) — та же c-shared обёртка вокруг
#   yggdrasil-go, что libygg.so;
# - yggdrasil.exe и yggdrasilctl.exe — узел и его утилита из той же версии
#   yggdrasil-go, что в ygg/go.mod (библиотека и узел не расходятся по протоколу).
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

# Узел: исходники той версии, что в go.mod, — копией из кэша модулей (он только для
# чтения). Чистый Go, cgo не нужен; версия вшивается так же, как в сборке upstream.
VER="$(go list -m -f '{{.Version}}' github.com/yggdrasil-network/yggdrasil-go)"
SRC="$(go list -m -f '{{.Dir}}' github.com/yggdrasil-network/yggdrasil-go)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -r "$SRC/." "$TMP" && chmod -R u+w "$TMP"
PKG=github.com/yggdrasil-network/yggdrasil-go/src/version
for cmd in yggdrasil yggdrasilctl; do
  (cd "$TMP" && CGO_ENABLED=0 go build -trimpath     -ldflags "-s -w -X $PKG.buildName=$cmd -X $PKG.buildVersion=${VER#v}"     -o "$OUT/$cmd.exe" "./cmd/$cmd")
done

echo "built: $OUT (yggdrasil-go $VER)"
ls -la "$OUT"
