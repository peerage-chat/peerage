#!/usr/bin/env python3
"""Заметки релиза GitHub из журнала приложения (п. 51, docs/release.md).

    git show v0.4.0:android/app/src/main/assets/changelog/en.json | changelog-notes.py 0.4.0

Читает журнал со stdin (берётся с тега, не из рабочей копии) и печатает в stdout markdown:
краткое описание, «New», «Fixed». Нет записи этой версии — код 1: релиз без журнала не
выпускается, иначе «Что нового» в приложении молчит.
"""
import json
import sys


def main() -> int:
    version = sys.argv[1]
    data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    entry = next((v for v in data.get("versions", []) if v.get("version") == version), None)
    if entry is None or not entry.get("summary"):
        print(f"в журнале нет записи {version}", file=sys.stderr)
        return 1
    out = [entry["summary"], ""]
    for title, key in (("New", "new"), ("Fixed", "fixed")):
        items = entry.get(key) or []
        if items:
            out += [f"### {title}", ""] + [f"- {i}" for i in items] + [""]
    sys.stdout.buffer.write("\n".join(out).encode("utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
