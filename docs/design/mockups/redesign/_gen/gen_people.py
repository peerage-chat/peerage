import json, os, random, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
ob = open(os.path.join(HERE, "gen_onboard.py"), encoding="utf-8").read()
# CSS, иконки, page, шапка, кнопки — из генератора первого запуска.
exec(ob[:ob.index("# ---------- 1. Приветствие ----------")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.hdr{display:flex;align-items:center;gap:6px;padding:14px 12px 10px 8px}
.hdr b{flex:1;font-size:22px;font-weight:700;color:#ECEDEF}
.sec{font-size:13px;font-weight:500;color:#8E8E98;padding:0 6px 8px}
.cam{position:relative;height:300px;border-radius:28px;overflow:hidden;background:linear-gradient(160deg,#2B3440,#151A20 60%,#202830)}
.cam i{position:absolute;width:44px;height:44px;border:3px solid #FFFFFF;border-radius:4px}
.cam .h{position:absolute;left:0;right:0;bottom:16px;text-align:center;font-size:14px;color:#FFFFFF}
.pr{display:flex;align-items:center;gap:14px;min-height:60px;padding:8px 16px;box-sizing:border-box}
.pr + .pr{box-shadow:inset 68px 1px 0 #1C1D22, inset 0 1px 0 #26272D}
.pr .t{flex:1;min-width:0}
.pr b{display:block;font-size:15px;font-weight:500;color:#ECEDEF}
.pr span{display:block;font-size:13px;color:#8E8E98;margin-top:2px}
.pr.dis b{color:#7A7A84}
.chip{display:flex;align-items:center;gap:6px;min-height:36px;padding:0 6px 0 4px;border-radius:18px;background:#26272D;font-size:14px;color:#ECEDEF;flex:none}
.chip .av{width:28px;height:28px;font-size:12px}
.srch{display:flex;align-items:center;gap:10px;min-height:46px;padding:0 14px;border-radius:16px;background:#1C1D22;color:#8E8E98;font-size:15px}
.qr{width:232px;height:232px;padding:16px;border-radius:24px;background:#FFFFFF;box-sizing:content-box}
.tag{font-size:12px;color:#8E8E98;background:#26272D;border-radius:10px;padding:3px 8px;flex:none}
"""

C = {"А": "#3949AB", "Б": "#00897B", "В": "#8E24AA", "Г": "#F4511E", "Д": "#6D4C41", "Е": "#546E7A", "Ж": "#C0CA33"}
PEOPLE = [("Алиса", "в сети"), ("Борис", "в сети вчера"), ("Вера", "в сети"), ("Глеб", "в сети 3 дня назад"),
          ("Дина", "в сети"), ("Егор", "в сети сегодня"), ("Жанна", "в сети неделю назад")]
SHARE = '<circle cx="6" cy="12" r="2.5"></circle><circle cx="18" cy="6" r="2.5"></circle><circle cx="18" cy="18" r="2.5"></circle><path d="M8.2 10.8l7.6-3.6M8.2 13.2l7.6 3.6"></path>'
QRI = '<path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4z"></path><path d="M14 14h2v2h-2zM18 14h2v2M14 18h2v2M18 18h2v2h-2"></path>'
USERS = '<circle cx="9" cy="8" r="3.5"></circle><path d="M3 20c.8-3.5 3.2-5.5 6-5.5s5.2 2 6 5.5"></path><path d="M16 4.5a3.5 3.5 0 0 1 0 7M18 14.5c1.6.8 2.6 2.7 3 5.5"></path>'
CROWN = '<path d="M4 8l4 4 4-7 4 7 4-4-2 11H6z"></path>'
MSG = '<path d="M5 5h14a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9l-4 3V6a1 1 0 0 1 1-1z"></path>'


def hdr(title, right=""):
    return '<div class="hdr">' + btn(BACK, "Назад") + f'<b>{title}</b>{right}</div>'


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
    return LIST_OPEN + f'<div class="ob" style="padding:{pad}px 10px 0;">{inner}</div>'


def av(name, size=40, sq=False):
    r = "14px" if sq else "50%"
    return (f'<div class="av" style="width:{size}px;height:{size}px;border-radius:{r};background:{C[name[0]]};'
            f'font-size:{size * 0.4:.0f}px;">{name[0]}</div>')


def check(on):
    return f'<div class="rc on">{ic(CHECK, 14, "#04222E")}</div>' if on else '<div class="rc"></div>'


def prow(name, sub, right="", cls=""):
    return (f'<div class="pr {cls}">{av(name)}<div class="t"><b>{name}</b>'
            + (f'<span>{sub}</span>' if sub else '') + f'</div>{right}</div>')


def qr_svg():
    rnd = random.Random(7)
    n = 29
    cell = 8
    rects = []
    for y in range(n):
        for x in range(n):
            finder = any(fx <= x < fx + 7 and fy <= y < fy + 7 for fx, fy in [(0, 0), (n - 7, 0), (0, n - 7)])
            if finder:
                continue
            if rnd.random() < 0.48:
                rects.append(f'<rect x="{x * cell}" y="{y * cell}" width="{cell}" height="{cell}"></rect>')
    for fx, fy in [(0, 0), (n - 7, 0), (0, n - 7)]:
        X, Y = fx * cell, fy * cell
        rects.append(f'<rect x="{X}" y="{Y}" width="56" height="56"></rect><rect x="{X + 8}" y="{Y + 8}" width="40" height="40" fill="#FFFFFF"></rect><rect x="{X + 16}" y="{Y + 16}" width="24" height="24"></rect>')
    return (f'<div class="qr"><svg width="232" height="232" viewBox="0 0 {n * cell} {n * cell}" fill="#08090B" role="img" '
            f'aria-label="QR-код обмена">{"".join(rects)}</svg></div>')


CORNERS = ('<i style="left:56px;top:56px;border-right:none;border-bottom:none;"></i>'
           '<i style="right:56px;top:56px;border-left:none;border-bottom:none;"></i>'
           '<i style="left:56px;bottom:56px;border-right:none;border-top:none;"></i>'
           '<i style="right:56px;bottom:56px;border-left:none;border-top:none;"></i>')

# ---------- 1. Добавить контакт ----------
add = page("Добавить контакт", hdr("Добавить контакт") + body(
    '<div class="cam" role="img" aria-label="Видоискатель камеры">' + CORNERS
    + '<div class="h">Навести на код собеседника</div></div>' + SP16
    + '<div class="grp">'
    + grow(PASTE, "Вставить код", "Если код прислали сообщением", '')
    + grow(QRI, "Показать свой код", "", '')
    + '</div>'
    + '<div class="cap" style="margin-top:8px;">Код передаётся лицом к лицу или любым мессенджером: в нём ключ, адрес в сети и имя.</div>'
    + FILL))

# ---------- 2. Код распознан ----------
found = page("Добавить контакт: код распознан", hdr("Добавить контакт") + body(
    '<div style="display:flex;align-items:center;gap:14px;padding:8px 0 20px;">'
    + av("Алиса", 64) + '<div><div class="obt" style="margin:0;font-size:22px;">Алиса</div>'
    + '<div class="cap" style="font-variant-numeric:tabular-nums;padding:0;">ключ 3f9a1c07…</div></div></div>'
    + '<div class="sec">Приветствие</div>'
    + '<div class="inp" style="align-items:flex-start;min-height:96px;padding-top:14px;"><span class="v" style="line-height:22px;">Привет! Это Аня, с курсов по керамике<span class="cr"></span></span></div>'
    + '<div class="cap" style="margin-top:8px;">Уйдёт запросом в контакты вместе с именем этого профиля; фото — после принятия.</div>'
    + SP16 + '<div class="sec">Как подписать у себя</div>'
    + '<div class="inp"><span class="p">Алиса</span></div>'
    + '<div class="cap" style="margin-top:8px;">Необязательно. Видно только на этом телефоне.</div>'
    + FILL) + foot("Отправить запрос", "Сканировать другой"))

# ---------- 2б. Запрос отправлен: чат ----------
WAIT = ('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#9FC6D6" stroke-width="2" '
        'stroke-linecap="round" aria-label="Ждёт" role="img"><circle cx="12" cy="12" r="9"></circle><path d="M12 7v5l3 2"></path></svg>')
pending = page("Запрос отправлен: чат", chat_header("Алиса", "запрос отправлен") + PANEL
               + '<div style="flex:1;display:flex;flex-direction:column;justify-content:flex-end;gap:3px;padding:12px 14px;">'
               + '<div class="lbl">Сегодня</div>'
               + '<div style="align-self:center;max-width:300px;text-align:center;font-size:13px;line-height:18px;color:#B8B8C0;background:rgba(255,255,255,0.06);padding:8px 12px;border-radius:14px;margin:6px 0 10px;">Запрос в контакты отправлен. Сообщения ниже Алиса получит, когда примет его.</div>'
               + f'<div class="bout o1">Привет! Это Аня, с курсов по керамике<span class="mto">14:31 {WAIT}</span></div>'
               + f'<div class="bout o2">Скину фото с занятия, как добавишь<span class="mto">14:32 {WAIT}</span></div>'
               + '</div>' + composer() + '</div>')

# ---------- 3. Ошибки кода ----------
def err(title, sub):
    return (f'<div class="state" style="display:flex;gap:12px;padding:14px 16px;border-radius:18px;background:#1C1D22;">'
            f'{ic(WARN, 22, "#F2C98A", NF)}<div><b style="display:block;font-size:15px;font-weight:500;color:#ECEDEF;">{title}</b>'
            f'<span style="display:block;font-size:13px;line-height:18px;color:#8E8E98;margin-top:2px;">{sub}</span></div></div>')


errs = page("Добавить контакт: что не так с кодом", hdr("Добавить контакт") + body(
    '<div class="cam" style="height:220px;" role="img" aria-label="Видоискатель камеры">' + CORNERS.replace("56px", "40px") + '</div>' + SP16
    + '<div style="display:flex;flex-direction:column;gap:8px;">'
    + err("Это не код Peerage", "QR распознан, но в нём нет ключа. Ссылки и другие коды здесь не открываются.")
    + err("Это код этого телефона", "Себя в контакты не добавить — нужен код собеседника.")
    + '<div style="display:flex;align-items:center;gap:12px;padding:12px 12px 12px 16px;border-radius:18px;background:#1C1D22;">' + av("Борис")
    + '<div style="flex:1;"><b style="display:block;font-size:15px;font-weight:500;color:#ECEDEF;">Борис уже в контактах</b></div>'
    + '<button class="pill">Открыть чат</button></div>'
    + '</div>' + FILL))

# ---------- 4. Мой код ----------
SCAN_I = '<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M4 12h16"></path>'
mycode = page("Мой код", hdr("Мой код") + body(
    '<div style="display:flex;flex-direction:column;align-items:center;gap:10px;padding:12px 0 20px;">'
    + qr_svg() + '<div class="obt" style="margin:12px 0 0;font-size:22px;">Аня</div>'
    + '<div class="cap" style="text-align:center;">Кто отсканирует код, добавит этот телефон в контакты.</div></div>'
    + '<div style="display:flex;gap:8px;">'
    + f'<button class="btn" style="flex:1;">{ic(COPY, 20, "#ECEDEF")}Скопировать</button>'
    + f'<button class="btn" style="flex:1;">{ic(SHARE, 20, "#ECEDEF")}Поделиться</button></div>'
    + f'<button class="btn" style="margin-top:8px;">{ic(SCAN_I, 20, "#ECEDEF")}Сканировать код</button>'
    + '<div class="cap" style="margin-top:12px;">В коде — ключ, адрес в сети и имя.</div>'
    + FILL))

# ---------- 5. Свой профиль ----------
avatar = ('<div style="position:relative;width:104px;height:104px;margin:16px auto 24px;">'
          + '<div style="width:104px;height:104px;border-radius:50%;background:#C2185B;color:#FFFFFF;font-size:42px;font-weight:500;display:flex;align-items:center;justify-content:center;">А</div>'
          + f'<div style="position:absolute;right:-2px;bottom:-2px;width:36px;height:36px;border-radius:50%;background:#33B5E5;box-shadow:0 0 0 3px #08090B;display:flex;align-items:center;justify-content:center;">{ic(CAMERA, 18, "#04222E")}</div></div>')
me_edit = page("Свой профиль: правка", hdr("Профиль", '<button class="pill" style="margin-right:4px;">Готово</button>') + body(
    avatar + '<div class="sec">Имя</div><div class="inp"><span class="v">Аня<span class="cr"></span></span></div>'
    + '<div class="cap" style="margin-top:8px;">Имя и фото рассылаются контактам. Контакт может задать этому телефону своё имя, оно заменит присланное.</div>'
    + SP16 + '<div class="grp">' + grow(TRASH, "Убрать фото", None, '') + '</div>'
    + FILL))

# ---------- 6. Новая группа · Б · сначала участники ----------
chips = '<div style="display:flex;gap:6px;overflow:hidden;padding-bottom:12px;">' + ''.join(
    f'<div class="chip">{av(n, 28)}{n}{ic(CLOSE, 14, "#9AA3AD", NF)}</div>' for n in ["Алиса", "Вера"]) + '</div>'
rows6 = ''.join(prow(n, s, check(n in ("Алиса", "Вера"))) for n, s in PEOPLE[:6])
g1 = page("Новая группа: участники", hdr("Новая группа", '<span class="cap" style="padding-right:12px;">2 выбрано</span>') + body(
    chips + f'<div class="srch">{ic(SEARCH, 20, "#8E8E98", NF)}Поиск по контактам</div>' + '<div style="height:12px;"></div>'
    + f'<div class="grp">{rows6}</div>' + FILL) + foot("Далее"))

# ---------- 7. Новая группа · имя ----------
g2 = page("Новая группа: название", hdr("Новая группа") + body(
    '<div style="display:flex;align-items:center;gap:16px;padding:12px 0 20px;">'
    + f'<div style="width:72px;height:72px;border-radius:22px;background:#26272D;display:flex;align-items:center;justify-content:center;flex:none;">{ic(CAMERA, 28, "#9AA3AD")}</div>'
    + '<div class="inp" style="flex:1;"><span class="v">Дача<span class="cr"></span></span></div></div>'
    + '<div class="sec">Участники · 3</div><div class="grp">'
    + prow("Аня", "создатель — админ", '<span class="tag">админ</span>').replace(C["А"], "#C2185B", 1)
    + prow("Алиса", "получит приглашение") + prow("Вера", "получит приглашение") + '</div>'
    + '<div class="cap" style="margin-top:8px;">Участники получат приглашение. Приглашать, удалять участников и закреплять сообщения может только админ.</div>'
    + FILL) + foot("Создать"))

# ---------- 8. Пригласить в группу ----------
rows8 = (prow("Алиса", "уже в группе", '', 'dis') + prow("Борис", "приглашён · ждёт ответа", '<span class="tag">приглашён</span>', 'dis')
         + prow("Глеб", "в сети 3 дня назад", check(True)) + prow("Дина", "в сети", check(False))
         + prow("Егор", "в сети сегодня", check(True)))
addm = page("Пригласить в группу", hdr("Пригласить", '<span class="cap" style="padding-right:12px;">2 выбрано</span>') + body(
    f'<div class="srch">{ic(SEARCH, 20, "#8E8E98", NF)}Поиск по контактам</div>' + '<div style="height:12px;"></div>'
    + f'<div class="grp">{rows8}</div>'
    + '<div class="cap" style="margin-top:8px;">Приглашаются только контакты. Вход в группу — после принятия приглашения.</div>'
    + FILL) + foot("Пригласить · 2"))

# ---------- 9. Участник: шторка админа ----------
behind = hdr("Дача") + body('<div class="grp">' + prow("Алиса", "в сети") + prow("Вера", "в сети") + prow("Глеб", "в сети 3 дня назад") + '</div>' + FILL)
sheet = page("Участник: действия админа", behind + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
             + '<div style="display:flex;align-items:center;gap:12px;padding:6px 4px 4px;">' + av("Вера", 48)
             + '<div><div class="sht" style="padding:0;font-size:18px;">Вера</div><div class="sub">в сети · в контактах</div></div></div>'
             + '<div class="menu" style="box-shadow:none;background:#26272D;">'
             + f'<button class="mi">{ic(MSG, 20, "#9AA3AD", NF)}Написать</button>'
             + f'<button class="mi">{ic(USER, 20, "#9AA3AD", NF)}Профиль</button>'
             + f'<button class="mi">{ic(CROWN, 20, "#9AA3AD", NF)}Передать права админа</button>'
             + '<div class="mdv"></div>'
             + f'<button class="mi red">{ic(TRASH, 20, "#FF6B6B", NF)}Удалить из группы</button></div>'
             + '<div class="sub">Удалённый участник перестаёт расшифровывать новые сообщения сразу: группа меняет ключи.</div></div>')

# ---------- 10. Выход админа ----------
leave = page("Выход админа из группы", behind + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
             + '<div class="sht">Выйти из «Дачи»?</div>'
             + '<div class="sub" style="font-size:14px;line-height:20px;">Этот телефон — админ группы. Без админа никто не сможет удалять участников и закреплять сообщения.</div>'
             + '<div class="grp" style="background:#26272D;">' + prow("Алиса", "в сети", check(True)) + prow("Вера", "в сети", check(False)) + '</div>'
             + '<button class="btn red">Передать Алисе и выйти</button>'
             + '<button class="btn">Выйти без передачи</button></div>')

# ---------- 11. Правка группы ----------
gedit = page("Группа: правка", hdr("Группа", '<button class="pill" style="margin-right:4px;">Готово</button>') + body(
    f'<div style="width:104px;height:104px;border-radius:32px;background:#00897B;color:#FFFFFF;font-size:42px;font-weight:500;display:flex;align-items:center;justify-content:center;margin:16px auto 24px;position:relative;">Д'
    + f'<div style="position:absolute;right:-4px;bottom:-4px;width:36px;height:36px;border-radius:50%;background:#33B5E5;box-shadow:0 0 0 3px #08090B;display:flex;align-items:center;justify-content:center;">{ic(CAMERA, 18, "#04222E")}</div></div>'
    + '<div class="sec">Название</div><div class="inp"><span class="v">Дача<span class="cr"></span></span></div>'
    + '<div class="cap" style="margin-top:8px;">Только админ. Новое название и фото получат все участники.</div>' + FILL))

# ---------- 12. Новая группа · А · сначала название ----------
g_a = page("Новая группа: А, сначала название", hdr("Новая группа") + body(
    '<div class="sec" style="margin-top:8px;">Название</div><div class="inp"><span class="v">Дача<span class="cr"></span></span></div>'
    + '<div class="cap" style="margin-top:8px;">Участники добавляются из группы.</div>' + FILL) + foot("Создать"))

rows = [
    [("Main.dc.html", add, "Добавить контакт: сразу камера"),
     ("Found.dc.html", found, "✓ Код распознан: приветствие = запрос"),
     ("Pending.dc.html", pending, "Запрос отправлен: чат ждёт принятия"),
     ("Errors.dc.html", errs, "Что не так с кодом"),
     ("MyCode.dc.html", mycode, "Мой код: без адреса и знаков"),
     ("MeEdit.dc.html", me_edit, "Свой профиль: имя и фото")],
    [("GroupPick.dc.html", g1, "Новая группа · Б ✓ Сначала участники"),
     ("GroupName.dc.html", g2, "Новая группа · Б: название, кто админ"),
     ("Invite.dc.html", addm, "Пригласить: несколько сразу, статусы"),
     ("Member.dc.html", sheet, "Участник: шторка админа"),
     ("Leave.dc.html", leave, "✓ Выход админа: передать права"),
     ("GroupEdit.dc.html", gedit, "Правка группы (админ): название, фото")],
    [("GroupA.dc.html", g_a, "Новая группа · А: сначала название — отвергнут")],
]
row_titles = ["Контакты и свой профиль", "Группы", "Вариант"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 2860}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(close_panel(html))
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-02T00:10:00Z"}, "title": "Контакты и группы Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
