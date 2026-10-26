#!/usr/bin/env bash
# Подтверждение: два процесса (как два десктопа) обмениваются через реальную сеть
# и СОХРАНЯЮТ историю на диск. Сверяем messages.json с обеих сторон.
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
cargo build -q -p peerage-messenger --examples
CHAT="$CARGO_TARGET_DIR/debug/examples/chat"
PC="$CARGO_TARGET_DIR/debug/examples/persisted_chat"

SEED_A=$(printf '0a%.0s' $(seq 1 32))
SEED_B=$(printf '0b%.0s' $(seq 1 32))
PUB_A=$("$CHAT" key "$SEED_A")
PUB_B=$("$CHAT" key "$SEED_B")

DA=$(mktemp -d); DB=$(mktemp -d)
echo "A=$PUB_A  data=$DA"
echo "B=$PUB_B  data=$DB"

"$PC" "$SEED_A" "$PUB_B" "$DA" peers/public-peers.txt A "сообщение от A (persist)" >/tmp/pc_a.log 2>&1 &
PA=$!
"$PC" "$SEED_B" "$PUB_A" "$DB" peers/public-peers.txt B "сообщение от B (persist)" >/tmp/pc_b.log 2>&1 &
PB=$!
RA=0; RB=0
wait $PA || RA=$?
wait $PB || RB=$?

echo "----- A log -----"; cat /tmp/pc_a.log
echo "----- B log -----"; cat /tmp/pc_b.log
echo "===== messages.json у A ====="; cat "$DA/messages.json"
echo "===== messages.json у B ====="; cat "$DB/messages.json"

ok=1
for d in "$DA" "$DB"; do
  mine=$(grep -c '"mine": true'  "$d/messages.json" || true)
  theirs=$(grep -c '"mine": false' "$d/messages.json" || true)
  deliv=$(grep -c '"delivered"' "$d/messages.json" || true)
  echo "$d: исходящих=$mine входящих=$theirs delivered=$deliv"
  [ "$mine" -ge 1 ] && [ "$theirs" -ge 1 ] && [ "$deliv" -ge 1 ] || ok=0
done
rm -rf "$DA" "$DB"
if [ $RA -eq 0 ] && [ $RB -eq 0 ] && [ $ok -eq 1 ]; then
  echo "✓✓✓ Подтверждено: две копии видят друг друга и СОХРАНЯЮТ диалог на диск"
else
  echo "✗ не подтверждено (RA=$RA RB=$RB ok=$ok)"; exit 1
fi
