#!/usr/bin/env python3
"""Собрать набор эмодзи панели: android/app/src/main/assets/emoji.tsv.

Источники (скачать заново при новой версии Unicode, положить в одну папку):
  emoji-test.txt         https://unicode.org/Public/emoji/latest/emoji-test.txt
  ann_{ru,en}.json       cldr-json: cldr-annotations-full/annotations/<lang>/annotations.json
  der_{ru,en}.json       cldr-json: cldr-annotations-derived-full/annotationsDerived/<lang>/annotations.json

Запуск: python scripts/gen-emoji.py <папка-с-источниками>

Берутся только полностью квалифицированные эмодзи без оттенков кожи (оттенки — потом, долгим
тапом) и без «компонентов». Строка: группа, эмодзи, названия (ru, en) и ключевые слова (ru, en),
списки через «|».
Каких символов нет в шрифте телефона, панель отсеивает сама (Paint.hasGlyph).
"""
import json
import sys
from pathlib import Path

GROUPS = {
    "Smileys & Emotion": 0,
    "People & Body": 1,
    "Animals & Nature": 2,
    "Food & Drink": 3,
    "Activities": 4,
    "Travel & Places": 5,
    "Objects": 6,
    "Symbols": 7,
    "Flags": 8,
}
SKIN = set(range(0x1F3FB, 0x1F400))


def annotations(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    return data[next(k for k in data if k.startswith("annotations"))]["annotations"]


def main(src: Path, out: Path) -> None:
    ann = {}
    for lang in ("ru", "en"):
        merged = {}
        for name in (f"ann_{lang}.json", f"der_{lang}.json"):
            merged.update(annotations(src / name))
        ann[lang] = merged
    version = "?"
    group = None
    rows = []
    for line in (src / "emoji-test.txt").read_text(encoding="utf-8").splitlines():
        if line.startswith("# Version:"):
            version = line.split(":", 1)[1].strip()
        if line.startswith("# group:"):
            group = GROUPS.get(line.split(":", 1)[1].strip())
            continue
        if group is None or not line or line.startswith("#") or "; fully-qualified" not in line:
            continue
        cps = [int(c, 16) for c in line.split(";")[0].split()]
        if any(c in SKIN for c in cps):
            continue
        emoji = "".join(chr(c) for c in cps)
        key = emoji.replace("️", "")
        names, words = [], []
        for field, bucket in (("tts", names), ("default", words)):
            for lang in ("ru", "en"):
                a = ann[lang].get(key) or ann[lang].get(emoji) or {}
                for w in a.get(field, []):
                    w = w.strip().lower().replace("|", " ").replace("\t", " ")
                    if w and w not in names and w not in words:
                        bucket.append(w)
        if not names:
            # Имя из комментария emoji-test: «E1.0 grinning face».
            names.append(line.split("#", 1)[1].split(" ", 3)[-1].strip().lower())
        rows.append(f"{group}\t{emoji}\t{'|'.join(names)}\t{'|'.join(words)}")
    head = [
        f"# Эмодзи панели: Unicode Emoji {version}, слова — Unicode CLDR (ru, en). Собрано scripts/gen-emoji.py.",
        "# Данные Unicode: Copyright © 1991-2026 Unicode, Inc. Unicode License v3, https://www.unicode.org/license.txt",
    ]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(head + rows) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(rows)} эмодзи, {out.stat().st_size // 1024} КБ → {out}")


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    main(Path(sys.argv[1]), root / "android/app/src/main/assets/emoji.tsv")
