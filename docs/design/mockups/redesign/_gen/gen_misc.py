import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
pp = open(os.path.join(HERE, "gen_people.py"), encoding="utf-8").read()
# Всё общее (CSS, иконки, page, шапка, панель «листа», строки людей) — из генератора контактов.
exec(pp[:pp.index("CORNERS = (")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.tiles{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px}
.tl2{background:#1C1D22;border-radius:18px;padding:12px 14px;display:flex;flex-direction:column;gap:2px}
.tl2 b{font-size:20px;font-weight:700;color:#ECEDEF;font-variant-numeric:tabular-nums}
.tl2 span{font-size:12px;color:#8E8E98}
.hl{background:rgba(51,181,229,0.28);color:#EAF6FB;border-radius:4px;padding:0 2px}
.note{align-self:flex-end;max-width:290px;background:#1F4657;color:#EAF6FB;font-size:15px;line-height:21px;padding:7px 12px;border-radius:18px}
.st{display:flex;align-items:center;gap:12px;min-height:44px;font-size:15px;color:#ECEDEF}
.st i{width:24px;height:24px;border-radius:50%;flex:none;display:flex;align-items:center;justify-content:center;font-style:normal;font-size:12px;font-weight:700}
.st.done i{background:#17261A}.st.now i{background:#13313D}.st.wait i{background:#26272D;color:#8E8E98}
.st.wait{color:#8E8E98}
"""

OUTBOX = '<path d="M4 13h4l2 3h4l2-3h4"></path><path d="M4 13l2-7h12l2 7v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z"></path><path d="M12 3v7M9.5 7.5L12 10l2.5-2.5"></path>'
BAN = '<circle cx="12" cy="12" r="9"></circle><path d="M5.6 5.6l12.8 12.8"></path>'
WIFI_OFF = '<path d="M3 3l18 18"></path><path d="M8.5 16.5a5 5 0 0 1 7 0M5 12.5a10 10 0 0 1 5-2.7M14 9.8a10 10 0 0 1 5 2.7M2 8.8a15 15 0 0 1 4.5-2.8M12 20h.01"></path>'
SWAP = '<path d="M4 8h13l-3-3M20 16H7l3 3"></path>'
FOLDER = '<path d="M4 6a1 1 0 0 1 1-1h4l2 2h8a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z"></path>'
BRIEF = '<rect x="3" y="7" width="18" height="13" rx="2"></rect><path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M3 13h18"></path>'
HOME = '<path d="M4 11l8-7 8 7v8a1 1 0 0 1-1 1h-4v-6H9v6H5a1 1 0 0 1-1-1z"></path>'
MSGS = '<path d="M4 5h12a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H8l-4 3V6a1 1 0 0 1 1-1z"></path><path d="M20 9v10l-3-2h-6"></path>'
HEADPH = '<path d="M4 15v-3a8 8 0 0 1 16 0v3"></path><rect x="3" y="14" width="4" height="6" rx="1.5"></rect><rect x="17" y="14" width="4" height="6" rx="1.5"></rect>'
SPEAK = '<path d="M4 10v4h3l7 4V6L7 10z"></path><path d="M18 9a4 4 0 0 1 0 6"></path>'
CHEVR = '<path d="M9 6l6 6-6 6"></path>'


def toggle(on):
    return f'<div class="tg{" on" if on else ""}"><i></i></div>'


def chev():
    return ic(CHEVR, 18, "#6B6C74", NF)


def tiles(items):
    return '<div class="tiles">' + ''.join(f'<div class="tl2"><b>{a}</b><span>{b}</span></div>' for a, b in items) + '</div>'


def keyrow(label, sub, right=""):
    return (f'<div class="pr"><div class="av" style="background:#26272D;color:#9AA3AD;">{ic(KEY, 20, "#9AA3AD")}</div>'
            f'<div class="t"><b style="font-variant-numeric:tabular-nums;">{label}</b><span>{sub}</span></div>{right}</div>')


# ---------- 1. Сохранённое ----------
saved = page("Сохранённое", hdr("Сохранённое", btn(SEARCH, "Поиск")) + LIST_OPEN
             + '<div style="flex:1;display:flex;flex-direction:column;justify-content:flex-end;gap:3px;padding:12px 14px;">'
             + '<div class="lbl">Вчера</div>'
             + '<div class="note o1">Код от подъезда 4417<span class="mto">21:05</span></div>'
             + '<div class="note o2">Купить: фильтр для воды, лампочки E14<span class="mto">21:06</span></div>'
             + '<div class="lbl">Сегодня</div>'
             + '<div class="note o1"><div class="q"><div style="color:#9AD7EE;font-weight:500;">Переслано · Алиса</div>Адрес мастерской: Литейный, 12, второй двор</div><span class="mto">09:40</span></div>'
             + '</div>'
             + composer(ic(CLIP, 20, "#9AA3AD", NF) + '<span class="ph">Заметка себе…</span>' + ic(SMILE, 20, "#9AA3AD", NF)))

# ---------- 2. Исходящие ----------
orow = lambda n, sub, extra='': prow(n, sub, extra + chev())
outbox = page("Исходящие", hdr("Исходящие") + body(
    tiles([("1 248", "сообщений"), ("3,4 МБ", "объём"), ("24 дня", "самое старое")])
    + '<div class="cap" style="margin:10px 0 16px;">Уйдут, как только получатель появится в сети. Сервера, который подержал бы их, нет — поэтому очередь сама не чистится.</div>'
    + f'<div class="warn" style="margin-bottom:16px;">{ic(WARN, 20, "#F2C98A", NF)}<div><b style="display:block;font-weight:500;">Очередь разрослась</b>Больше тысячи неотправленных — обычно получатель давно не появлялся в сети.</div></div>'
    + '<div class="sec">По получателям</div><div class="grp">'
    + orow("Глеб", "1 204 · 3,2 МБ · не в сети 24 дня")
    + orow("Борис", "31 · 120 КБ · отправляется…")
    + orow("Вера", "13 · 64 КБ · не в сети 2 часа") + '</div>'
    + '<div class="cap" style="margin-top:8px;">Тап — открыть чат или очистить очередь получателя.</div>' + FILL)
    + '<div class="obf"><button class="btn red">Очистить всё</button></div>')

# ---------- 3. Подтверждение удержанием ----------
hold = page("Исходящие: очистить всё", hdr("Исходящие") + body(tiles([("1 248", "сообщений"), ("3,4 МБ", "объём"), ("24 дня", "самое старое")]) + FILL)
            + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
            + '<div class="sht">Очистить всю очередь?</div>'
            + '<div class="sub" style="font-size:14px;line-height:20px;">1 248 сообщений для Глеба, Бориса и Веры не дойдут никогда. Это сообщения с этого телефона, которые ещё никто не получил; вернуть их будет неоткуда.</div>'
            + '<button class="btn red" style="position:relative;overflow:hidden;"><span style="position:absolute;left:0;top:0;bottom:0;width:58%;background:rgba(255,107,107,0.25);"></span><span style="position:relative;">Удерживать, чтобы очистить</span></button>'
            + '<button class="btn">Не очищать</button></div>')

# ---------- 4. Общий поиск ----------
def mres(chat, when, snippet, sq=False):
    return (f'<div class="pr">{av(chat, 40, sq)}<div class="t"><b style="display:flex;justify-content:space-between;">{chat}'
            f'<span style="margin:0;font-weight:400;">{when}</span></b><span style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">{snippet}</span></div></div>')


search = page("Поиск", '<div class="hdr">' + btn(BACK, "Назад")
              + f'<div class="srch" style="flex:1;color:#ECEDEF;">{ic(SEARCH, 20, "#9AA3AD", NF)}керами<span class="cr"></span></div></div>'
              + body('<div class="sec">Контакты и группы</div><div class="grp">'
                     + prow("Дина", "в группе «Керамика по средам»")
                     + f'<div class="pr">{av("Жанна", 40, True).replace(C["Ж"], "#00897B")}<div class="t"><b>Керамика по средам</b><span>группа · 6 участников</span></div></div>'
                     + '</div>' + SP16 + '<div class="sec">Сообщения · 14</div><div class="grp">'
                     + mres("Алиса", "вчера", 'Привет! Это Аня, с курсов по <span class="hl">керами</span>ке')
                     + mres("Вера", "28 окт", 'Глина для <span class="hl">керами</span>ки приехала, забирай')
                     + mres("Дина", "12 окт", 'Курс <span class="hl">керами</span>ки переносят на четверг')
                     + '</div><div class="cap" style="margin-top:8px;">Поиск идёт по сообщениям, которые хранятся на этом устройстве; запрос никуда не отправляется.</div>'
                     + FILL))

# ---------- 5. Папки ----------
def frow(icon, name, sub, right=""):
    return (f'<div class="gr">{ic(icon, 22, "#9AA3AD", NF)}<div class="gt"><b>{name}</b>'
            + (f'<span>{sub}</span>' if sub else '') + f'</div>{right}</div>')


folders = page("Папки", hdr("Папки") + body(
    '<div class="cap" style="margin-bottom:16px;">Папка — это вид, а не коробка: один чат может попадать сразу в несколько, удаление папки переписку не трогает.</div>'
    + '<div class="sec">Встроенные</div><div class="grp">'
    + frow(MSGS, "Все", None) + frow(MSG, "Личные", None) + frow(HEADPH, "Комнаты", None) + frow(SPEAK, "Каналы", None) + '</div>'
    + SP16 + '<div class="sec">Свои</div><div class="grp">'
    + frow(BRIEF, "Работа", "7 чатов", chev()) + frow(HOME, "Семья", "4 чата", chev())
    + f'<div class="gr">{ic(PLUS, 22, "#33B5E5", NF)}<div class="gt"><b style="color:#33B5E5;">Создать папку</b></div></div>'
    + '</div>' + FILL))

# ---------- 6. Редактор папки ----------
icons = ''.join(f'<div style="width:44px;height:44px;border-radius:14px;background:{"#13313D" if i == 0 else "#26272D"};display:flex;align-items:center;justify-content:center;{"box-shadow:inset 0 0 0 1.5px #33B5E5;" if i == 0 else ""}">{ic(p, 20, "#7FD0EE" if i == 0 else "#9AA3AD")}</div>'
                for i, p in enumerate([BRIEF, HOME, FOLDER, HEADPH, SPEAK, MSG]))
editor = page("Папка: правка", hdr("Папка", '<span class="cap" style="padding-right:12px;color:#7FD0EE;">7 чатов</span>') + body(
    '<div class="inp"><span class="v">Работа<span class="cr"></span></span></div>'
    + f'<div style="display:flex;gap:8px;margin:10px 0 18px;">{icons}</div>'
    + '<div class="sec">Включить</div>'
    + '<div style="display:flex;gap:8px;margin-bottom:10px;"><button class="tb on">Личные</button><button class="tb">Комнаты</button></div>'
    + '<div class="sec">И конкретные чаты</div><div class="grp">'
    + prow("Борис", None, check(True)) + f'<div class="pr">{av("Жанна", 40, True).replace(C["Ж"], "#00897B")}<div class="t"><b>Керамика по средам</b></div>{check(False)}</div>'
    + '</div>' + SP16 + '<div class="sec">Исключить</div><div class="grp">'
    + frow(CHECK, "Прочитанные", None, toggle(False)) + '</div>'
    + FILL) + '<div class="obf"><button class="btn ghost" style="color:#FF8A8A;">Удалить папку</button></div>')

# ---------- 7. Заблокированные ----------
UNB = '<button class="pill" style="background:#26272D;color:#C4C7CC;">Разблокировать</button>'
blocked = page("Заблокированные", hdr("Заблокированные") + body(
    '<div class="grp">'
    + prow("Глеб", "ключ 7c21e0a4… · с 28 октября", UNB)
    + keyrow("ключ 9b0e5f12…", "не был в контактах · с 30 октября", UNB)
    + '</div><div class="cap" style="margin-top:8px;">Пакеты заблокированных отбрасываются до расшифровки. Разблокированный не возвращается в контакты сам: связаться снова — только новым запросом.</div>'
    + FILL))

# ---------- 8. Поток трафика ----------
flood = page("Поток трафика", hdr("Поток трафика") + body(
    tiles([("1 840", "пакетов в секунду"), ("2,1 млн", "отброшено"), ("2", "источника")])
    + '<div class="cap" style="margin:10px 0 16px;">Пакеты отбрасываются до расшифровки — процессор, батарея и память не тратятся. Но трафик уже потрачен: остановить чужие пакеты на телефоне нельзя.</div>'
    + '<div class="sec">Источники</div><div class="grp">'
    + keyrow("ключ 9b0e5f12…", "не в контактах · 1 790 пкт/с")
    + prow("Егор", "заблокирован · 50 пкт/с", '<span class="tag">заблокирован</span>') + '</div>'
    + SP16 + '<div class="sec">Что можно сделать</div><div class="grp">'
    + frow(WIFI_OFF, "Уйти в офлайн", "Трафик перестанет тратиться сразу. Сообщения не приходят, пока связь не вернуть.", '<button class="pill" style="background:#26272D;color:#C4C7CC;">В офлайн</button>')
    + frow(SWAP, "Сменить адрес", "Единственный способ оборвать поток и остаться в сети. Контактам уйдёт новый адрес.", '<button class="pill">Сменить</button>')
    + '</div>' + FILL))

# ---------- 9. Сменить адрес ----------
rot = page("Сменить адрес", hdr("Сменить адрес") + body(
    '<div class="cap" style="margin-bottom:14px;">Личность не меняется: имя, ключ, история и группы остаются. Меняется только адрес в сети.</div>'
    + '<div class="sec" style="display:flex;justify-content:space-between;">Сообщить новый адрес<span>4 из 5</span></div><div class="grp">'
    + prow("Алиса", "в сети", check(True)) + prow("Борис", "узнает, когда появится в сети", check(True))
    + prow("Вера", "в сети", check(True)) + prow("Дина", "в сети", check(True))
    + keyrow("ключ 9b0e5f12…", "источник потока — снят", check(False)) + '</div>'
    + '<div class="cap" style="margin-top:8px;">Не отмеченные останутся со старым адресом и потеряют связь с этим телефоном.</div>'
    + SP16 + '<div class="sec">Как это произойдёт</div><div class="grp" style="padding:4px 16px;">'
    + '<div class="st wait"><i>1</i>Минута офлайн — старый адрес исчезает из сети</div>'
    + '<div class="st wait"><i>2</i>Подъём под новым адресом</div>'
    + '<div class="st wait"><i>3</i>Отмеченным — подписанное уведомление</div></div>'
    + FILL) + foot("Сменить адрес", None, '<div class="cap" style="text-align:center;padding-bottom:4px;">Отменить нельзя: старый адрес освобождается сразу.</div>'))

# ---------- 10. Ход смены ----------
prog = page("Сменить адрес: ход", hdr("Сменить адрес") + body(
    '<div class="grp" style="padding:4px 16px;">'
    + f'<div class="st done"><i>{ic(CHECK, 14, "#8BC34A")}</i>Старый адрес погашен</div>'
    + '<div class="st now"><i style="color:#7FD0EE;">2</i>Пауза — осталось 42 с</div>'
    + '<div class="st wait"><i>3</i>Подъём под новым адресом</div>'
    + '<div class="st wait"><i>4</i>Уведомлено 0 из 4</div></div>'
    + '<div class="cap" style="margin-top:10px;">Телефон сейчас офлайн. Сообщения, отправленные в эти секунды, дойдут позже — отправители повторят их сами.</div>'
    + FILL))

# ---------- 11. Копия данных: состав ----------
def part(name, sub, on):
    return f'<div class="gr"><div class="gt"><b>{name}</b><span>{sub}</span></div>{toggle(on)}</div>'


bparts = page("Копия данных: состав", hdr("Копия данных") + body(
    f'<div class="grp">{grow(ARCHIVE, "Копия переписки, а не личности", "Личность возвращают 24 слова. Копия — то, что читать: контакты, сообщения, заметки.", "")}</div>'
    + SP16 + '<div class="sec">Что положить в копию</div><div class="grp">'
    + part("Контакты", "23 · с аватарами", True) + part("Переписка", "4 812 сообщений", True)
    + part("Групповые чаты", "1 250 сообщений", True) + part("Заметки себе", "37", True)
    + part("Журнал звонков", "120 звонков", False) + part("Вложения", "214 файлов · 1,2 ГБ", False) + '</div>'
    + '<div class="cap" style="margin-top:8px;">Членство в группах не входит: копия возвращает историю групп, писать в них — после нового приглашения.</div>'
    + FILL) + foot("Далее", "Восстановить из копии"))

# ---------- 12. Копия данных: пароль ----------
bpass = page("Копия данных: пароль", hdr("Копия данных") + body(
    '<div class="sec">Отдельный пароль</div>'
    + '<div class="cap" style="margin-bottom:12px;">Файл уедет с телефона — в облако, на карту, на чужой компьютер. Ключа этого устройства там нет, поэтому у копии свой пароль.</div>'
    + '<div class="inp"><span class="v">••••••••••••</span></div><div style="height:8px;"></div>'
    + '<div class="inp"><span class="p">Ещё раз</span></div>' + SP16
    + f'<div class="warn">{ic(WARN, 20, "#F2C98A", NF)}<div>Забытый пароль не восстанавливается: копия станет файлом, который никто не откроет.</div></div>'
    + FILL) + foot("Создать копию", None, '<div class="cap" style="text-align:center;padding-bottom:4px;">Дальше система спросит, куда сохранить файл.</div>'))

# ---------- 13. Копия данных: готово ----------
bdone = page("Копия данных: готово", hdr("Копия данных") + body(
    f'<div style="display:flex;flex-direction:column;align-items:center;gap:10px;padding:24px 0;">{ic(CHECK, 44, "#8BC34A")}'
    + '<div class="obt" style="margin:0;font-size:24px;">Копия готова</div><div class="cap">backup-2026-11-01.peerage · 48 МБ</div></div>'
    + '<div class="grp">' + ''.join(f'<div class="gr"><div class="gt"><b>{a}</b></div><span style="color:#8E8E98;font-size:15px;font-variant-numeric:tabular-nums;">{b}</span></div>'
                                    for a, b in [("Контакты", "23"), ("Сообщения", "4 812"), ("Групповые", "1 250"), ("Заметки", "37")]) + '</div>'
    + '<div class="cap" style="margin-top:8px;">Отметки «сверен» в копию не входят — после восстановления контакты сверяются заново.</div>'
    + FILL) + foot("Готово"))

rows = [
    [("Main.dc.html", saved, "Сохранённое: заметки себе"),
     ("Outbox.dc.html", outbox, "Исходящие: сводка, по получателям"),
     ("OutboxHold.dc.html", hold, "Очистить всё — удержанием"),
     ("Search.dc.html", search, "Общий поиск: люди, группы, сообщения")],
    [("Folders.dc.html", folders, "Папки"),
     ("FolderEdit.dc.html", editor, "Папка: правка, живой счётчик"),
     ("Blocked.dc.html", blocked, "✓ Заблокированные: имя хранится при блокировке")],
    [("Flood.dc.html", flood, "Поток трафика"),
     ("Rotate.dc.html", rot, "Сменить адрес: кому сообщить"),
     ("RotateProgress.dc.html", prog, "Смена адреса: ход")],
    [("BackupParts.dc.html", bparts, "Копия данных: состав"),
     ("BackupPass.dc.html", bpass, "Копия данных: пароль"),
     ("BackupDone.dc.html", bdone, "Копия данных: готово")],
]
row_titles = ["Сохранённое, исходящие, поиск", "Папки и блокировки", "Поток трафика и смена адреса", "Копия данных"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 1890}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(close_panel(html))
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-02T00:40:00Z"}, "title": "Второстепенные экраны Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
