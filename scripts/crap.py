#!/usr/bin/env python3
"""CRAP по функциям: сложность из rust-code-analysis, покрытие строк из LCOV.

CRAP = c² · (1 − p)³ + c, где c — цикломатическая сложность функции (с вложенными
замыканиями), p — доля её исполнимых строк, которые прошли тесты. Функция с CRAP > 10
либо слишком ветвиста, либо мало проверена: разбить, покрыть или то и другое.

Зовётся из scripts/crap.sh; сам по себе: crap.py <lcov> <корень> [порог].
"""

import json
import subprocess
import sys
from pathlib import Path


def parse_lcov(text):
    """{файл: {строка: число попаданий}} из LCOV."""
    hits, cur = {}, None
    for line in text.splitlines():
        if line.startswith("SF:"):
            cur = hits.setdefault(line[3:], {})
        elif line.startswith("DA:") and cur is not None:
            n, count = line[3:].split(",")[:2]
            cur[int(n)] = int(count)
    return hits


def functions(space):
    """(имя, первая строка, последняя, сложность) именованных функций дерева.
    Замыкания не отдельно — они входят в сложность объемлющей."""
    if space["kind"] == "function" and space["name"] != "<anonymous>":
        yield (
            space["name"],
            space["start_line"],
            space["end_line"],
            space["metrics"]["cyclomatic"]["sum"],
        )
    for child in space.get("spaces", []):
        yield from functions(child)


def tests_start(source):
    """Первая строка модуля тестов (`#[cfg(test)]` перед `mod tests`) или `None`:
    rust-code-analysis модулей не выделяет, а тесты в отчёте — шум."""
    lines = source.splitlines()
    for i, (attr, decl) in enumerate(zip(lines, lines[1:])):
        if attr.strip() == "#[cfg(test)]" and decl.strip().startswith("mod tests"):
            return i + 1
    return None


def crap(complexity, covered):
    return complexity**2 * (1 - covered) ** 3 + complexity


def score(fns, lines):
    """[(crap, сложность, покрытие, имя, строка)] для функций файла; `lines` —
    попадания по строкам. Функция без исполнимых строк в LCOV не оценивается."""
    out = []
    for name, start, end, c in fns:
        body = [h for n, h in lines.items() if start <= n <= end]
        if not body:
            continue
        p = sum(1 for h in body if h > 0) / len(body)
        out.append((crap(c, p), c, p, name, start))
    return out


def analyze(path):
    raw = subprocess.run(
        ["rust-code-analysis-cli", "-m", "-O", "json", "-p", str(path)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    cut = tests_start(Path(path).read_text())
    return [f for f in functions(json.loads(raw)) if cut is None or f[1] < cut]


def main(lcov_path, root, limit=10.0):
    root = Path(root).resolve()
    rows = []
    for file, lines in parse_lcov(Path(lcov_path).read_text()).items():
        path = Path(file)
        if root not in path.parents or "tests" in path.relative_to(root).parts:
            continue
        for row in score(analyze(path), lines):
            rows.append((*row, path.relative_to(root)))
    rows.sort(reverse=True)
    bad = [r for r in rows if r[0] > limit]
    for value, c, p, name, line, path in bad:
        print(f"{value:7.1f}  сложн. {c:4.0f}  покр. {p:4.0%}  {path}:{line}  {name}")
    print(f"функций: {len(rows)}, CRAP > {limit:g}: {len(bad)}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], *(float(a) for a in sys.argv[3:4]))
