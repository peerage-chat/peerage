#!/usr/bin/env python3
# Генерирует простую RGBA PNG-иконку (Holo-синий круг на тёмном фоне)
# средствами только stdlib (struct + zlib).
import struct, zlib, os, math

W = H = 256
bg = (26, 26, 26, 255)       # #1a1a1a
fg = (51, 181, 229, 255)     # #33b5e5
cx, cy, r = W / 2, H / 2, 90

raw = bytearray()
for y in range(H):
    raw.append(0)  # filter type 0
    for x in range(W):
        d = math.hypot(x - cx, y - cy)
        # кольцо + точка в центре (стилизованный «узел сети»)
        if abs(d - r) < 10 or d < 22:
            px = fg
        else:
            px = bg
        raw.extend(px)

def chunk(typ, data):
    c = struct.pack(">I", len(data)) + typ + data
    return c + struct.pack(">I", zlib.crc32(typ + data) & 0xffffffff)

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")

out = os.path.join(os.path.dirname(__file__), "..", "desktop", "icons", "icon.png")
os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "wb") as f:
    f.write(png)
print("written", os.path.abspath(out), len(png), "bytes")
