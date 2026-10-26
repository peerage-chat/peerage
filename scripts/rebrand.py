#!/usr/bin/env python3
"""Переименовать продукт целиком: scripts/rebrand.py <Name> <slug> <app.id>

Пример: python3 scripts/rebrand.py Peerage peerage chat.peerage

Текущее имя берётся из brand.env. Меняется всё, что несёт имя (docs/BRAND.md):
видимое название, идентификатор приложения и пакет Kotlin (с переносом каталогов и
JNI-символами), крейты `<slug>-*`, библиотека, документация.

НЕ меняются метки протокола `<slug>:<домен>:vN` — от них выводятся ключи, и смена
имени не должна ломать шифрование и старые данные. Они остаются со старым именем
навсегда (как «Whisper» в метках Signal).

После запуска: пересобрать .so (scripts/rebuild-android.sh <ABI>) и APK, прогнать
тесты. Смена идентификатора приложения = новая установка на телефоне.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKIP_PREFIX = ("docs/archive/", "target", ".git/")


def read_brand() -> dict:
    out = {}
    for line in open(os.path.join(ROOT, "brand.env"), encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            out[k] = v
    return out


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    new_name, new_slug, new_id = sys.argv[1:]
    if not re.fullmatch(r"[a-z][a-z0-9]*", new_slug):
        sys.exit("slug: только строчные латинские буквы и цифры")
    # Сегменты пакета без «_» и заглавных: иначе JNI-имена кодируются иначе (_1…).
    if not re.fullmatch(r"[a-z][a-z0-9]*(\.[a-z][a-z0-9]*)+", new_id):
        sys.exit("app.id: сегменты из строчных букв и цифр, через точку")
    old = read_brand()
    old_name, old_slug, old_id = old["BRAND_NAME"], old["BRAND_SLUG"], old["APP_ID"]
    if (old_name, old_slug, old_id) == (new_name, new_slug, new_id):
        sys.exit("имя и так такое")

    label = re.compile(re.escape(old_slug) + r":[a-z]")
    rules = [
        (old_id.replace(".", "_"), new_id.replace(".", "_")),  # JNI: Java_chat_peerage_…
        (old_id, new_id),  # пакет и идентификатор
        (old_id.replace(".", "/"), new_id.replace(".", "/")),  # пути к исходникам
        (old_name, new_name),
        (old_slug.upper(), new_slug.upper()),
        (old_slug, new_slug),
    ]

    os.chdir(ROOT)
    files = subprocess.run(["git", "ls-files"], capture_output=True, text=True, check=True).stdout.splitlines()
    changed = 0
    for f in files:
        if f.startswith(SKIP_PREFIX) or not os.path.isfile(f):
            continue
        try:
            # newline="": концы строк (CRLF/LF) остаются как были.
            text = open(f, encoding="utf-8", newline="").read()
        except (UnicodeDecodeError, OSError):
            continue
        # Метки протокола прячем на время замены — их имя не трогаем.
        held = []

        def hold(m):
            held.append(m.group(0))
            return f"\x00{len(held) - 1}\x00"

        new = label.sub(hold, text)
        for a, b in rules:
            new = new.replace(a, b)
        new = re.sub("\x00(\\d+)\x00", lambda m: held[int(m.group(1))], new)
        if new != text:
            open(f, "w", encoding="utf-8", newline="").write(new)
            changed += 1

    src_old, src_new = old_id.replace(".", "/"), new_id.replace(".", "/")
    for base in ("android/app/src/main/java", "android/app/src/test/java"):
        a, b = f"{base}/{src_old}", f"{base}/{src_new}"
        if os.path.isdir(a):
            os.makedirs(os.path.dirname(b), exist_ok=True)
            subprocess.run(["git", "mv", a, b], check=True)
            # Пустые каталоги старого пакета (chat/ после chat/peerage) убираем.
            d = os.path.dirname(a)
            while d != base and os.path.isdir(d) and not os.listdir(d):
                os.rmdir(d)
                d = os.path.dirname(d)

    print(f"{old_name} -> {new_name}: изменено файлов {changed}")


if __name__ == "__main__":
    main()
