#!/usr/bin/env bash
# Кросс-проверка вывода адреса Yggdrasil: эталон yggdrasil-go vs наша Rust-реализация.
# Запуск из WSL:  bash scripts/validate-ygg-addr.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

cd "$ROOT/ygg"
go mod tidy >/dev/null 2>&1
OUT=$(go run ./cmd/yggaddr)
PUB=$(printf '%s\n'  "$OUT" | awk '/^pubkey/{print $2}')
GOADDR=$(printf '%s\n' "$OUT" | awk '/^addr/{print $2}')

echo "pubkey      : $PUB"
echo "go addr     : $GOADDR"

# Rust-сторона: на Windows cargo может быть только в Windows-PATH.
# Если cargo доступен в WSL — посчитаем и сверим прямо здесь.
if command -v cargo >/dev/null 2>&1; then
  cd "$ROOT"
  RSADDR=$(cargo run -q -p peerage-core --example addr -- "$PUB")
  echo "rust addr   : $RSADDR"
  if [ "$GOADDR" = "$RSADDR" ]; then echo "MATCH ✓"; else echo "MISMATCH ✗"; exit 1; fi
else
  echo "(cargo нет в WSL — сверьте вручную: cargo run -p peerage-core --example addr -- $PUB)"
fi
