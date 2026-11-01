import json, math, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
ob = open(os.path.join(HERE, "gen_onboard.py"), encoding="utf-8").read()
# CSS, иконки, page, шапка, сетка 24 слов, клавиатура — из генератора первого запуска.
exec(ob[:ob.index("# ---------- 1. Приветствие ----------")])
exec(ob[ob.index("def phrase_grid(slots):"):ob.index("grid = phrase_grid(WORDS)")])
exec(ob[ob.index("def keyboard(sugs):"):ob.index("check = check_screen(")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.glyphs{display:grid;grid-template-columns:repeat(6,1fr);gap:8px}
.gl{aspect-ratio:1;border-radius:12px;background:#23242A;display:flex;align-items:center;justify-content:center}
.fpw{font-size:15px;line-height:22px;color:#7FD0EE;font-variant-numeric:tabular-nums}
.blk{background:#1C1D22;border-radius:18px;padding:14px 16px 16px;display:flex;flex-direction:column;gap:12px}
.bh{display:flex;align-items:center;gap:10px;font-size:15px;font-weight:500;color:#ECEDEF}
.bh span{margin-left:auto;font-size:13px;font-weight:400;color:#8E8E98}
.state{display:flex;align-items:center;gap:12px;padding:12px 16px;border-radius:18px}
.state b{display:block;font-size:15px;font-weight:500}
.state span{display:block;font-size:13px;line-height:18px;color:#B8B8C0;margin-top:2px}
.radio{display:flex;align-items:flex-start;gap:14px;padding:14px 16px}
.radio + .radio{box-shadow:inset 0 1px 0 #26272D}
.radio b{display:block;font-size:15px;font-weight:500;color:#ECEDEF}
.radio span{display:block;font-size:13px;line-height:18px;color:#8E8E98;margin-top:3px}
.radio .rc{margin-top:1px}
.risk{display:flex;gap:8px;margin-top:8px;font-size:13px;line-height:18px;color:#F2C98A}
.sub2{margin:0 16px 14px 52px;padding:12px 14px;border-radius:14px;background:#26272D;display:flex;align-items:center;gap:12px}
.sub2 b{flex:1;font-size:14px;font-weight:500;color:#ECEDEF}
.tg.on{background:#33B5E5}.tg.on i{left:21px;background:#04222E}
.bio{position:absolute;left:12px;right:12px;bottom:12px;background:#2A2B31;border-radius:28px;padding:24px 20px 16px;display:flex;flex-direction:column;align-items:center;gap:8px;text-align:center}
.bio b{font-size:18px;font-weight:500;color:#ECEDEF}
.bio span{font-size:14px;color:#B8B8C0}
.hdr{display:flex;align-items:center;gap:6px;padding:14px 12px 10px 8px}
.hdr b{font-size:20px;font-weight:700;color:#ECEDEF}
.sec{font-size:13px;font-weight:500;color:#8E8E98;padding:0 4px 8px}
"""

PAL = ["#4FC3F7", "#4DB6AC", "#81C784", "#AED581", "#FFD54F", "#FFB74D", "#FF8A65", "#E57373",
       "#F06292", "#BA68C8", "#9575CD", "#7986CB", "#90A4AE", "#A1887F", "#DCE775", "#FFFFFF"]


def poly(n, rot, r=10, cx=12, cy=12):
    pts = []
    for i in range(n):
        a = math.radians(rot + 360 * i / n)
        pts.append(f"{cx + r * math.cos(a):.1f},{cy + r * math.sin(a):.1f}")
    return " ".join(pts)


def star(n, r=10, cx=12, cy=12):
    pts = []
    for i in range(n * 2):
        rr = r if i % 2 == 0 else r * 0.45
        a = math.radians(-90 + 180 * i / n)
        pts.append(f"{cx + rr * math.cos(a):.1f},{cy + rr * math.sin(a):.1f}")
    return " ".join(pts)


def glyph(b):
    """Тот же знак, что рисует FingerprintGrid: форма — младшие 4 бита, цвет — старшие."""
    c = PAL[(b >> 4) & 15]
    k = b & 15
    f = f'fill="{c}"'
    s = f'fill="none" stroke="{c}" stroke-width="3"'
    shape = {
        0: f'<circle cx="12" cy="12" r="10" {f}></circle>',
        1: f'<circle cx="12" cy="12" r="8.5" {s}></circle>',
        2: f'<rect x="2" y="2" width="20" height="20" {f}></rect>',
        3: f'<rect x="3.5" y="3.5" width="17" height="17" {s}></rect>',
        4: f'<polygon points="{poly(3, -90)}" {f}></polygon>',
        5: f'<polygon points="{poly(3, 90)}" {f}></polygon>',
        6: f'<polygon points="{poly(4, -90)}" {f}></polygon>',
        7: f'<polygon points="{poly(5, -90)}" {f}></polygon>',
        8: f'<polygon points="{poly(6, -90)}" {f}></polygon>',
        9: f'<polygon points="{star(5)}" {f}></polygon>',
        10: f'<polygon points="{star(6)}" {f}></polygon>',
        11: f'<path d="M12 2v20M2 12h20" fill="none" stroke="{c}" stroke-width="4"></path>',
        12: f'<path d="M5 5l14 14M19 5L5 19" fill="none" stroke="{c}" stroke-width="4"></path>',
        13: f'<path d="M17.5 5.5A8.5 8.5 0 1 0 20.5 12" {s}></path>',
        14: f'<circle cx="12" cy="14.5" r="7" {f}></circle><polygon points="{poly(3, -90, 6, 12, 9)}" {f}></polygon>',
        15: f'<circle cx="12" cy="12" r="8.5" fill="none" stroke="{c}" stroke-width="2.5"></circle><circle cx="12" cy="12" r="3.5" {f}></circle>',
    }[k]
    return f'<div class="gl"><svg width="26" height="26" viewBox="0 0 24 24" aria-hidden="true">{shape}</svg></div>'


def glyphs(bs):
    return '<div class="glyphs" role="img" aria-label="Отпечаток ключа: 12 знаков">' + ''.join(glyph(b) for b in bs) + '</div>'


FP_ALICE = [0x29, 0x84, 0x1E, 0xC2, 0x57, 0xA0, 0x3B, 0xF6, 0x6D, 0x15, 0x98, 0x4C]
FP_SELF = [0x72, 0x0F, 0xB9, 0x46, 0xE3, 0x1A, 0x5C, 0x88, 0xD1, 0x27, 0x9E, 0x60]
FP_PAIR = [0x5A, 0xC7, 0x13, 0x8E, 0x31, 0xB4, 0x6F, 0x02, 0xE9, 0x4D, 0x95, 0x2B]
W_ALICE = "copper · rival · onion · tribe · fame · lucky · siren · wolf"
W_SELF = "ladder · humble · night · pause · garlic · vessel · bamboo · roof"
W_PAIR = "mixture · cabin · orbit · sail · tenant · jewel · forum · drip"

QR = '<path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4z"></path><path d="M14 14h2v2h-2zM18 14h2v2M14 18h2v2M18 18h2v2h-2"></path>'
SCAN = '<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M4 12h16"></path>'
VER = '<path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"></path><path d="M9 12l2 2 4-4"></path>'
MAYBE = '<path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"></path><path d="M10 9.5a2 2 0 1 1 2.5 2c-.4.2-.5.6-.5 1M12 15.5h.01"></path>'
FACE = '<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3"></path><path d="M9 10h.01M15 10h.01M9.5 15a3.5 3.5 0 0 0 5 0"></path>'

FINGER = '<path d="M8 11a4 4 0 0 1 8 0v2a8 8 0 0 1-1.5 4.7M12 11v3a6 6 0 0 1-2 4.5M5.5 8.5A7 7 0 0 1 19 10v2M5 13v-1.5M8.5 19.5A9 9 0 0 0 9 14v-2.5"></path>'


def hdr(title):
    return '<div class="hdr">' + btn(BACK, "Назад") + f'<b>{title}</b></div>'


LIST_OPEN = ('<!--panel--><div style="flex:1;min-height:0;background:#16171B;border-radius:28px 28px 0 0;'
             'display:flex;flex-direction:column;overflow:hidden;">')


def close_panel(html):
    """Экран «листа»: панель открывается в body() и закрывается здесь, после подвала."""
    if "<!--panel-->" not in html:
        return html
    i = html.rindex("\n</div>\n</x-dc>")
    return html[:i] + "</div>" + html[i:]


def body(inner, pad=16):
    """Содержимое на панели «листа» (шапка — на фоне, над ней)."""
    return LIST_OPEN + f'<div class="ob" style="padding-top:{pad}px;">{inner}</div>'


def state(icon, color, bg, title, sub):
    return (f'<div class="state" style="background:{bg};">{ic(icon, 24, color, NF)}'
            f'<div><b style="color:{color};">{title}</b><span>{sub}</span></div></div>')


UNVER = state(MAYBE, "#F2C98A", "#2A2116", "Ключ не сверен",
              "Переписка зашифрована, но не подтверждено, что на том конце именно Алиса.")
ACTIONS = ('<div style="display:flex;gap:8px;">'
           + f'<button class="btn pri" style="flex:1;">{ic(SCAN, 20, "#04222E")}Сканировать код</button>'
           + f'<button class="btn" style="flex:1;">{ic(QR, 20, "#ECEDEF")}Свой код</button></div>')

# ---------- 1. А · два ключа ----------
va = page("Сверка ключа: А, два ключа", hdr("Сверка ключа") + body(
    UNVER + SP16
    + '<div class="blk"><div class="bh">Алиса<span>на этом телефоне</span></div>' + glyphs(FP_ALICE)
    + f'<div class="fpw">{W_ALICE}</div></div>' + SP16
    + '<div class="blk"><div class="bh">Свой ключ<span>у Алисы — в её профиле</span></div>' + glyphs(FP_SELF)
    + f'<div class="fpw">{W_SELF}</div></div>'
    + '<div class="cap" style="margin-top:8px;">Блок «Алиса» здесь должен совпасть с блоком «Свой ключ» на её телефоне, и наоборот.</div>'
    + FILL) + '<div class="obf">' + ACTIONS + '<button class="btn ghost">Отметить сверенным без кода</button></div>')

# ---------- 2. Б · один набор на пару ----------
vb = page("Сверка ключа: Б, общий набор", hdr("Сверка ключа") + body(
    UNVER + SP16
    + '<div class="blk"><div class="bh">Алиса и этот телефон<span>одинаково у обоих</span></div>' + glyphs(FP_PAIR)
    + f'<div class="fpw">{W_PAIR}</div></div>'
    + '<div class="cap" style="margin-top:8px;">Тот же набор Алиса видит у себя на экране сверки. Совпало — ключи настоящие. Можно сверить голосом по видеосвязи: прочитать слова.</div>'
    + FILL) + '<div class="obf">' + ACTIONS + '<button class="btn ghost">Набор совпал — отметить сверенным</button></div>')

# ---------- 3. Код совпал ----------
behind = hdr("Сверка ключа") + body(UNVER + SP16 + '<div class="blk">' + glyphs(FP_PAIR) + '</div>' + FILL)
ok_sheet = page("Сверка ключа: код совпал", behind + '<div class="dim"></div><div class="sheet">'
                + '<div class="hdl"></div>'
                + f'<div style="display:flex;flex-direction:column;align-items:center;gap:10px;padding:12px 0 4px;">{ic(VER, 48, "#8BC34A")}'
                + '<div class="sht" style="padding:0;">Ключ совпал</div>'
                + '<div class="sub" style="text-align:center;">Алиса отмечена сверенной на этом телефоне. Чтобы отметка появилась и у неё, ей нужно отсканировать код этого телефона.</div></div>'
                + f'<button class="btn">{ic(QR, 20, "#ECEDEF")}Показать свой код Алисе</button>'
                + '<button class="btn pri">Готово</button></div>')

# ---------- 4. Код не совпал ----------
bad_sheet = page("Сверка ключа: код не совпал", behind + '<div class="dim"></div><div class="sheet">'
                 + '<div class="hdl"></div>'
                 + f'<div style="display:flex;flex-direction:column;align-items:center;gap:10px;padding:12px 0 4px;">{ic(WARN, 48, "#FF8A8A")}'
                 + '<div class="sht" style="padding:0;color:#FFB4B4;">Ключ не совпал</div>'
                 + '<div class="sub" style="text-align:center;">Код на том телефоне принадлежит другому ключу. Это либо код другого человека, либо между телефонами посредник. Переписке с этим контактом не доверять, пока ключ не совпадёт.</div></div>'
                 + '<button class="btn">Сканировать ещё раз</button>'
                 + '<button class="btn pri">Понятно</button></div>')

# ---------- 5. Сверен ----------
verified = page("Сверка ключа: сверен", hdr("Сверка ключа") + body(
    state(VER, "#8BC34A", "#17261A", "Ключ сверен", "1 ноября, по коду. Связь с Алисой идёт без посредника.")
    + SP16 + '<div class="blk"><div class="bh">Алиса и этот телефон<span>одинаково у обоих</span></div>' + glyphs(FP_PAIR)
    + f'<div class="fpw">{W_PAIR}</div></div>' + FILL)
    + '<div class="obf"><button class="btn">Сверить заново</button><button class="btn ghost" style="color:#FF8A8A;">Снять отметку</button></div>')

# ---------- 6. Замок ----------
lock = page("Блокировка приложения", '<div class="ob" style="align-items:center;justify-content:center;text-align:center;gap:12px;">'
            + f'{MESH}<div class="obt" style="margin:8px 0 0;">Peerage заблокирован</div>'
            + '<div class="obs">Отпечаток, лицо или PIN телефона — то, что настроено в системе.</div></div>'
            + '<div class="obf"><button class="btn pri" style="min-height:52px;">' + ic(FINGER, 20, "#04222E") + 'Разблокировать</button></div>'
            + '<div class="dim"></div><div class="bio" role="dialog" aria-label="Системный запрос биометрии">'
            + ic(FINGER, 56, "#33B5E5") + '<b>Разблокировать Peerage</b><span>Приложите палец к сканеру</span>'
            + '<button class="btn ghost" style="margin-top:8px;">Использовать PIN</button></div>')

# ---------- 7. Пароль хранилища ----------
vault = page("Пароль хранилища", '<div style="height:72px;flex:none;"></div><div class="ob">'
             + heading("Данные зашифрованы", "Переписка на этом телефоне закрыта паролем. Пока он не введён, сообщения не принимаются.", ic(LOCK, 34, "#33B5E5"))
             + SP24 + '<div class="inp" style="box-shadow:inset 0 0 0 1.5px #FF6B6B;"><span class="v">•••••••<span class="cr"></span></span>' + ic(EYE_OFF, 20, "#9AA3AD", NF) + '</div>'
             + '<div class="cap" style="color:#FF8A8A;margin-top:8px;">Неверный пароль</div>'
             + FILL + '</div>' + foot("Открыть", "Пароль забыт"))

vault_bio = page("Пароль хранилища: отпечаток", '<div style="height:72px;flex:none;"></div><div class="ob">'
                 + heading("Данные зашифрованы", "Переписка на этом телефоне закрыта паролем.", ic(LOCK, 34, "#33B5E5"))
                 + FILL + '</div>' + foot("Ввести пароль", None)
                 + '<div class="dim"></div><div class="bio" role="dialog" aria-label="Системный запрос биометрии">'
                 + ic(FINGER, 56, "#33B5E5") + '<b>Открыть переписку</b><span>Приложите палец к сканеру</span>'
                 + '<button class="btn ghost" style="margin-top:8px;">Ввести пароль</button></div>')

# ---------- 8. Пароль забыт ----------
forgot = page("Пароль хранилища: забыт", '<div style="height:72px;flex:none;"></div><div class="ob">'
              + heading("Данные зашифрованы", None, ic(LOCK, 34, "#33B5E5")) + FILL + '</div>'
              + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
              + '<div class="sht">Пароль забыт</div>'
              + '<div class="sub" style="font-size:14px;line-height:20px;">Пароль не восстанавливается. Переписка на этом телефоне останется закрытой.</div>'
              + '<div class="grp" style="background:#26272D;">'
              + grow(KEY, "Личность — по 24 словам", "Ключ тот же; переписку и адрес в сети вернёт файл копии данных", '')
              + '</div>'
              + '<div class="sub">Закрытые данные откладываются, а не удаляются. После восстановления шифрование — «ключ устройства», пароль задаётся заново.</div>'
              + '<button class="btn pri">Восстановить по 24 словам</button>'
              + '<button class="btn">Вспомнить ещё раз</button></div>')

# ---------- 9. Уровень шифрования ----------
RISK = (f'<div class="risk">{ic(WARN, 16, "#F2C98A", NF)}<div>Система может сбросить ключ устройства — после смены '
        'блокировки экрана или обновления прошивки. Тогда переписка не откроется; вернуть её можно только из файла копии данных.</div></div>')


def radio(on, t, s, extra=""):
    mark = f'<div class="rc on">{ic(CHECK, 14, "#04222E")}</div>' if on else '<div class="rc"></div>'
    return f'<div class="radio">{mark}<div><b>{t}</b><span>{s}</span>{extra}</div></div>'


level = page("Шифрование данных", hdr("Шифрование данных") + body(
    '<div class="grp">'
    + radio(False, "Ключ устройства", "Открывается само. Снятая копия памяти бесполезна; на разблокированном телефоне от чужих рук защищает только блокировка приложения.", RISK)
    + radio(True, "Пароль", "Спрашивается при каждом запуске; перезапустит система приложение — сообщения ждут ввода. От сброса ключа устройства не зависит.")
    + '<div class="sub2">' + ic(FINGER, 20, "#9AA3AD", NF) + '<b>Открывать отпечатком</b><div class="tg on"><i></i></div></div>'
    + radio(False, "Ключ устройства и пароль", "Нужны и этот телефон, и пароль: снятая с него копия без пароля бесполезна.", RISK)
    + '</div>' + SP16 + '<div class="sec">Новый пароль</div>'
    + '<div class="inp"><span class="v">••••••••••</span></div><div style="height:8px;"></div>'
    + '<div class="inp"><span class="p">Повторить</span></div>' + SP16
    + f'<div class="warn">{ic(WARN, 20, "#F2C98A", NF)}<div>Забытый пароль не восстанавливается: личность вернут 24 слова, переписку — файл копии данных.</div></div>'
    + FILL) + foot("Применить"))

# ---------- 10. Показать ключ ----------
showkey = page("Безопасность: ключ", hdr("Ключ") + body(
    '<div class="obs" style="margin-bottom:12px;">Показано после разблокировки. Скриншот и запись экрана заблокированы.</div>'
    + phrase_grid(WORDS) + FILL) + foot("Проверить запись", None))

# ---------- 11. Данные не открываются ----------
recovery = page("Данные не открываются", '<div style="height:56px;flex:none;"></div><div class="ob">'
                + heading("Данные не открываются", "Ключ к переписке на этом телефоне недоступен.", ic(WARN, 34, "#F2C98A"))
                + SP24 + '<div class="grp">'
                + grow(SHIELD, "Причина", "Система сбросила ключ шифрования. Так бывает после смены блокировки экрана или обновления прошивки.", '')
                + '</div><div class="cap" style="margin-top:8px;">Прочитать переписку отсюда нельзя. Данные откладываются, а не удаляются. Личность возвращают 24 слова.</div>'
                + FILL + '</div>' + foot("Восстановить по 24 словам", "Начать заново — новая личность"))

rows = [
    [("Main.dc.html", va, "Сверка · А: два набора — отвергнут"),
     ("Pair.dc.html", vb, "Сверка · Б ✓ Один набор на пару"),
     ("Match.dc.html", ok_sheet, "✓ Код совпал: шторка, не тост"),
     ("Mismatch.dc.html", bad_sheet, "Код не совпал"),
     ("Verified.dc.html", verified, "Сверен: когда и как")],
    [("Lock.dc.html", lock, "Блокировка: сразу системная биометрия"),
     ("Vault.dc.html", vault, "Пароль хранилища при запуске"),
     ("VaultBio.dc.html", vault_bio, "Пароль хранилища: сначала отпечаток"),
     ("Forgot.dc.html", forgot, "✓ Пароль забыт: что остаётся"),
     ("Level.dc.html", level, "Уровень: риск сброса ключа + отпечаток"),
     ("ShowKey.dc.html", showkey, "Безопасность → Ключ (после разблокировки)"),
     ("Recovery.dc.html", recovery, "Данные не открываются")],
]
row_titles = ["Сверка ключа контакта", "Блокировка, шифрование, ключ"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 2860}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(close_panel(html))
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T23:30:00Z"}, "title": "Безопасность Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
