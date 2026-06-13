#!/usr/bin/env bash
# Реальная сеть: два отдельных процесса, подключённых ТОЛЬКО к публичным пирам
# (не друг к другу), обмениваются сообщениями через глобальный Yggdrasil-mesh.
# Запуск из WSL: bash scripts/run-realnet-chat.sh
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=target-wsl
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export LD_LIBRARY_PATH="$ROOT/ygg/libygg:${LD_LIBRARY_PATH:-}"

bash scripts/build-ygg.sh >/dev/null
cargo build -q -p p2p-messenger --example chat
CHAT="$CARGO_TARGET_DIR/debug/examples/chat"

# Детерминированные seed-ы → известные публичные ключи.
SEED_A=$(printf '01%.0s' $(seq 1 32))
SEED_B=$(printf '02%.0s' $(seq 1 32))
PUB_A=$("$CHAT" key "$SEED_A")
PUB_B=$("$CHAT" key "$SEED_B")
echo "A pubkey: $PUB_A"
echo "B pubkey: $PUB_B"
echo "Запускаем два независимых узла (пиринг только с публичной сетью)..."
echo

# Запускаем оба процесса параллельно.
"$CHAT" run "$SEED_A" "$PUB_B" peers/public-peers.txt A >/tmp/chat_a.log 2>&1 &
PID_A=$!
"$CHAT" run "$SEED_B" "$PUB_A" peers/public-peers.txt B >/tmp/chat_b.log 2>&1 &
PID_B=$!

RC_A=0; RC_B=0
wait $PID_A || RC_A=$?
wait $PID_B || RC_B=$?

echo "===== узел A ====="; cat /tmp/chat_a.log
echo "===== узел B ====="; cat /tmp/chat_b.log
echo
if [ $RC_A -eq 0 ] && [ $RC_B -eq 0 ]; then
  echo "✓✓✓ E2E чат работает ЧЕРЕЗ РЕАЛЬНУЮ ГЛОБАЛЬНУЮ СЕТЬ Yggdrasil"
else
  echo "✗ обмен не состоялся (A rc=$RC_A, B rc=$RC_B)"
  exit 1
fi
