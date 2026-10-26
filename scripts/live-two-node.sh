#!/usr/bin/env bash
# Живой прогон двух узлов на РЕАЛЬНОМ публичном Yggdrasil-меше (без устройств).
#
# Зачем: loopback-тесты не имеют настоящего MTU/маршрутизации, поэтому изменения
# в размерах пакетов (напр. паддинг M5) надо проверять на живом транспорте.
# Узел A принимает, узел B шлёт текст и файл; успех = у A появились события
# `text`, `media` и `blob_received` с верным размером.
#
# Запуск из WSL (нужна libygg):
#   bash scripts/live-two-node.sh [размер_файла_байт]
set -u
cd "$(dirname "$0")/.."
# Машинные настройки (напр. CARGO_TARGET_DIR на другом диске) — в scripts/local.env,
# он в .gitignore. Без него сборка идёт в ./target, как обычно.
[ -f scripts/local.env ] && . scripts/local.env
T="${CARGO_TARGET_DIR:-target}"
export LD_LIBRARY_PATH="$PWD/ygg/libygg"

SIZE="${1:-30000}"        # по умолчанию 2 полных чанка + хвост
WORK="$T/live-nodes"   # внутри target → не мусорим в репо и переживает /tmp
NODE="$T/debug/examples/mesh_node"

if [ ! -x "$NODE" ]; then
    echo "Собираю mesh_node…"
    cargo build -p peerage-engine --example mesh_node || exit 1
fi

rm -rf "$WORK"
mkdir -p "$WORK"
head -c "$SIZE" /dev/urandom > "$WORK/payload.bin"

# Идентичность стабильна по каталогу — снимаем ключи коротким первым запуском.
A=$(timeout 10 "$NODE" "$WORK/a" --name NodeA 2>/dev/null | grep -m1 '^PUB' | cut -d' ' -f2)
B=$(timeout 10 "$NODE" "$WORK/b" --name NodeB 2>/dev/null | grep -m1 '^PUB' | cut -d' ' -f2)
if [ -z "$A" ] || [ -z "$B" ]; then echo "FAIL: узлы не отдали ключи"; exit 1; fi
echo "A=$A"
echo "B=$B"

# Оба знают друг друга → гейт контактов пропускает сообщения сразу в чат.
timeout 80 "$NODE" "$WORK/a" --name NodeA --add "$B" > "$WORK/a.log" 2>&1 &
sleep 6
timeout 70 "$NODE" "$WORK/b" --name NodeB --add "$A" \
    --send-text 'live padding check' --send-media "$WORK/payload.bin" > "$WORK/b.log" 2>&1
wait

echo "=== RECEIVER (A) ==="
grep -E '^EVENT' "$WORK/a.log" | head -10

# Критерий успеха: текст дошёл и блоб собран ровно того размера, что слали.
if grep -q '"type":"text"' "$WORK/a.log" && grep -q "\"size\":$SIZE" "$WORK/a.log"; then
    echo "OK: текст доставлен, блоб собран ($SIZE Б) через реальный меш"
else
    echo "FAIL: ожидали событие text и blob_received size=$SIZE — см. $WORK/a.log"
    exit 1
fi
