import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
pp = open(os.path.join(HERE, "gen_people.py"), encoding="utf-8").read()
# Общее (CSS, иконки, page, шапка чата, лента, панель «листа») — из генератора контактов.
exec(pp[:pp.index("CORNERS = (")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.dlg{position:absolute;left:24px;right:24px;top:50%;transform:translateY(-50%);background:#1C1D22;border-radius:28px;padding:24px 20px 16px;display:flex;flex-direction:column;gap:10px;box-shadow:0 16px 40px rgba(0,0,0,0.5)}
.dlg b{font-size:20px;font-weight:700;color:#ECEDEF}
.snack{position:absolute;left:12px;right:12px;bottom:84px;min-height:52px;border-radius:16px;background:#2A2B31;display:flex;align-items:center;gap:12px;padding:0 8px 0 16px;box-shadow:0 8px 24px rgba(0,0,0,0.45)}
.snack span{flex:1;font-size:15px;color:#ECEDEF}
.snack button{min-height:40px;padding:0 12px;border:none;border-radius:12px;background:none;color:#33B5E5;font-family:inherit;font-size:15px;font-weight:500}
"""

MEDIA = '<rect x="4" y="5" width="16" height="14" rx="2"></rect><circle cx="9" cy="10" r="1.6"></circle><path d="M20 15l-4.5-4.5L7 19"></path>'
BELL_OFF = '<path d="M6 16V11a6 6 0 0 1 9.5-4.9M18 11v5l1.5 2H8"></path><path d="M10 20a2 2 0 0 0 4 0M3 3l18 18"></path>'
INFO = '<circle cx="12" cy="12" r="9"></circle><path d="M12 11v5M12 8h.01"></path>'
ERASE = '<path d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3"></path>'
BAN = '<circle cx="12" cy="12" r="9"></circle><path d="M5.6 5.6l12.8 12.8"></path>'
EXIT = '<path d="M14 4h4a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-4M10 16l-4-4 4-4M6 12h10"></path>'
ADDU = '<circle cx="10" cy="8" r="3.5"></circle><path d="M4 20c.8-3.5 3.2-5.5 6-5.5 1.3 0 2.5.4 3.5 1.1M18 14v6M15 17h6"></path>'


def menu_box(items, red, top=62):
    m = ''.join(f'<button class="mi">{ic(p, 20, "#9AA3AD", NF)}<span style="flex:1;">{t}</span>{r}</button>' for p, t, r in items)
    m += '<div class="mdv"></div>' + ''.join(f'<button class="mi red">{ic(p, 20, "#FF6B6B", NF)}{t}</button>' for p, t in red)
    return f'<div class="menu" style="position:absolute;right:12px;top:{top}px;width:248px;">{m}</div>'


chat_bg = chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3") + composer() + '</div>'
grp_bg = (chat_header("Дача", "6 участников · 3 в сети", "#00897B", sq=True) + PANEL
          + feed("in1", "in2", "out1", "out2", "in3") + composer() + '</div>')

# ---------- 1. «⋮» личного чата ----------
dm_menu = page("Меню чата: личный", chat_bg + menu_box(
    [(SEARCH, "Поиск", ""), (MEDIA, "Медиа и файлы", ""), (BELL_OFF, "Без звука", '<div class="tg"><i></i></div>'),
     (USER, "Профиль", "")],
    [(ERASE, "Очистить историю"), (BAN, "Заблокировать")]))

# ---------- 2. «⋮» группы ----------
gr_menu = page("Меню чата: группа", grp_bg + menu_box(
    [(SEARCH, "Поиск", ""), (MEDIA, "Медиа и файлы", ""), (BELL_OFF, "Без звука", '<div class="tg on"><i></i></div>'),
     (ADDU, "Пригласить", ""), (INFO, "О группе", "")],
    [(EXIT, "Выйти из группы")]))

# ---------- 3. Подтверждение · А · диалог по центру ----------
dlg = page("Подтверждение: А, диалог", chat_bg + '<div class="dim"></div><div class="dlg" role="dialog">'
           + '<b>Очистить историю с Алисой?</b>'
           + '<div class="sub" style="font-size:15px;line-height:21px;">Переписка удалится с этого телефона вместе с неотправленными сообщениями. У Алисы она останется.</div>'
           + '<div style="height:6px;"></div><button class="btn red">Очистить</button><button class="btn" style="background:none;">Отмена</button></div>')

# ---------- 4. Подтверждение · Б · шторка ----------
sheet_c = page("Подтверждение: Б, шторка", chat_bg + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
               + '<div class="sht">Очистить историю с Алисой?</div>'
               + '<div class="sub" style="font-size:15px;line-height:21px;">Переписка удалится с этого телефона вместе с неотправленными сообщениями. У Алисы она останется.</div>'
               + '<button class="btn red">Очистить</button><button class="btn">Отмена</button></div>')

# ---------- 5. «Отменить» после локального действия ----------
RING = ('<svg width="22" height="22" viewBox="0 0 24 24" aria-label="Осталось 3 секунды" role="img"><circle cx="12" cy="12" r="9" fill="none" stroke="#3A3B42" stroke-width="2.5"></circle>'
        '<circle cx="12" cy="12" r="9" fill="none" stroke="#33B5E5" stroke-width="2.5" stroke-dasharray="56.5" stroke-dashoffset="22" transform="rotate(-90 12 12)"></circle>'
        '<text x="12" y="16" text-anchor="middle" font-size="10" fill="#ECEDEF" font-family="Golos Text, sans-serif">3</text></svg>')
undo_bg = chat_header() + PANEL + feed("in1", "in2", "out2", "in3") + composer() + '</div>'
undo = page("Отменить: сообщение удалено", undo_bg
            + f'<div class="snack" role="status">{RING}<span>Сообщение удалено</span><button>Отменить</button></div>')

# ---------- 6. «Отменить» в списке ----------
undo2 = page("Отменить: чат убран из папки", '<div class="hdr">' + f'<b style="padding-left:12px;">Работа</b>' + btn(SEARCH, "Поиск") + '</div>' + body(
    '<div class="grp">' + prow("Борис", "ок, созвонимся в 11") + prow("Вера", "Глина приехала, забирай") + prow("Егор", "Отчёт в папке") + '</div>' + FILL)
    + f'<div class="snack" role="status" style="bottom:24px;">{RING}<span>Глеб убран из «Работы»</span><button>Отменить</button></div>')

rows = [
    [("Main.dc.html", dm_menu, "«⋮» личного чата"),
     ("GroupMenu.dc.html", gr_menu, "«⋮» группы")],
    [("ConfirmA.dc.html", dlg, "Подтверждение · А: диалог по центру"),
     ("ConfirmB.dc.html", sheet_c, "Подтверждение · Б: шторка снизу")],
    [("Undo.dc.html", undo, "«Отменить»: удалено у себя"),
     ("UndoList.dc.html", undo2, "«Отменить»: убрано из папки")],
]
row_titles = ["Меню чата", "Подтверждения", "Отмена действия"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 904}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(close_panel(html))
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-02T01:10:00Z"}, "title": "Общие элементы Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
