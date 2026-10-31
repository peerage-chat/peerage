#!/usr/bin/env python3
"""Журнал ревью docs/REVIEW.md — механическая часть.

    python scripts/review.py next                  — следующий свободный номер R-NNN
    python scripts/review.py open                  — открытые находки (строки сводки)
    python scripts/review.py close R-115 abc1234 [def5678 …]
                                                   — в сводке «открыто» → «исправлено (хэши)»

Текст «Исправлено (…): как» в разделе находки и запись в docs/BUGS.md пишутся
руками — это объяснение, а не механика (см. .claude/skills/review-close).
"""
import re
import subprocess
import sys
from pathlib import Path

REVIEW = Path(__file__).resolve().parent.parent / "docs" / "REVIEW.md"
ROW = re.compile(r"^\| (R-\d{3}) \|.*\| ([^|]+) \|$")


def rows(text):
    for i, line in enumerate(text.split("\n")):
        m = ROW.match(line)
        if m:
            yield i, m.group(1), m.group(2).strip()


def main(argv):
    text = REVIEW.read_text(encoding="utf-8")
    cmd = argv[1] if len(argv) > 1 else ""
    if cmd == "next":
        nums = [int(n) for n in re.findall(r"\bR-(\d{3})\b", text)]
        print(f"R-{max(nums) + 1:03d}")
    elif cmd == "open":
        for _, rid, status in rows(text):
            if status == "открыто":
                print(rid)
    elif cmd == "close" and len(argv) >= 4:
        rid, hashes = argv[2], argv[3:]
        for h in hashes:
            # Хэш должен быть настоящим коммитом — опечатка в журнале хуже пустоты.
            ok = subprocess.run(["git", "rev-parse", "--verify", "--quiet", h + "^{commit}"],
                                stdout=subprocess.DEVNULL).returncode == 0
            if not ok:
                sys.exit(f"{h}: нет такого коммита")
        lines = text.split("\n")
        hit = [i for i, r, s in rows(text) if r == rid]
        if not hit:
            sys.exit(f"{rid}: нет строки в сводке")
        i = hit[0]
        m = ROW.match(lines[i])
        if m.group(2).strip() != "открыто":
            sys.exit(f"{rid}: статус уже «{m.group(2).strip()}»")
        lines[i] = lines[i][: m.start(2)] + f"исправлено ({', '.join(hashes)}) |"
        REVIEW.write_text("\n".join(lines), encoding="utf-8", newline="\n")
        print(lines[i])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
