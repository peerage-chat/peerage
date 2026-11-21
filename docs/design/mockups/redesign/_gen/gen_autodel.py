import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
# Общие куски (CSS ленты и шторок, иконки, page) — из генератора «Особых».
src = open(os.path.join(HERE, "gen_special.py"), encoding="utf-8").read()
exec(src[:src.index("# ---------- 1. Статусы отправки")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.grp{background:#1C1D22;border-radius:18px;display:flex;flex-direction:column;overflow:hidden;flex:none}
.r{display:flex;align-items:center;gap:14px;min-height:56px;padding:9px 14px;background:none;border:none;width:100%;box-sizing:border-box;font-family:inherit;color:#ECEDEF;font-size:16px;text-align:left}
.ic{width:36px;height:36px;border-radius:12px;background:#26272D;display:flex;align-items:center;justify-content:center;flex:none}
.lb{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}
.v{color:#8E8E98;font-size:14px;flex:none}
.dv{height:1px;background:#26272D;margin-left:14px}
.hd{font-size:13px;font-weight:500;color:#8E8E98;padding:0 6px;margin-bottom:-6px}
.cap{font-size:13px;line-height:18px;color:#8E8E98;padding:0 6px}
.segs{display:flex;gap:4px;padding:4px;background:#26272D;border-radius:12px}
.sg{flex:1;height:36px;border-radius:9px;border:none;background:none;color:#B8B8C0;font-family:inherit;font-size:14px;font-weight:500}
.sg.on{background:#13313D;color:#7FD0EE}
.blk{display:flex;flex-direction:column;gap:10px;padding:14px}
.blk b{font-size:16px;font-weight:400;color:#ECEDEF}
.opt{display:flex;align-items:center;gap:14px;min-height:52px;padding:0 4px;font-size:16px;color:#ECEDEF}
.opt span{flex:1}
.opt small{display:block;font-size:13px;color:#8E8E98;margin-top:2px}
"""

CLOCKX = '<circle cx="12" cy="13" r="7"></circle><path d="M12 10v3l2 1.5M9 3h6"></path>'
BACKP = '<path d="M15 5l-7 7 7 7"></path>'


def head(title):
    return ('<div style="display:flex;align-items:center;gap:10px;padding:14px 12px 10px 8px;">'
            + f'<button class="ib" style="background:none;" aria-label="Назад">{ic(BACKP, 22)}</button>'
            + f'<div style="flex:1;font-size:22px;font-weight:700;color:#ECEDEF;">{title}</div></div>')


def panel(inner):
    return ('<div style="flex:1;min-height:0;background:#16171B;border-radius:28px 28px 0 0;display:flex;flex-direction:column;'
            f'gap:14px;padding:10px;overflow:hidden;">{inner}</div>')


def segs(options, on):
    return '<div class="segs">' + ''.join(f'<button class="sg{" on" if o == on else ""}">{o}</button>' for o in options) + '</div>'


def radio(on):
    return f'<div class="rc on">{ic(CHECK, 14, "#04222E")}</div>' if on else '<div class="rc"></div>'


# ---------- 1. Хранилище: вход ----------
st = open(os.path.join(HERE, "..", "screens", "Storage.dc.html"), encoding="utf-8").read()
row = ('<div class="dv"></div>\n<button class="r"><span class="ic">' + ic(CLOCKX, 20, "#C4C7CC")
       + '</span><span class="lb"><span>Автоудаление</span><span class="sub">Сообщения старше месяца · вложения от 10 МБ старше недели</span></span></button>\n')
anchor = '</div>\n\n<div class="hd">По чатам</div>'
assert st.count(anchor) == 1
storage = st.replace(anchor, row + anchor).replace("<title>Хранилище</title>", "<title>Хранилище: автоудаление</title>")

# ---------- 2. Экран правила ----------
rules = page("Автоудаление", head("Автоудаление") + panel(
    '<div class="cap">Удаление только на этом телефоне: у собеседников ничего не меняется. «Сохранённое» и закреплённые сообщения не удаляются.</div>'
    + '<div class="hd">Сообщения</div><div class="grp"><div class="blk"><b>Удалять старше</b>'
    + segs(["Никогда", "Недели", "Месяца", "Года"], "Месяца") + '</div></div>'
    + '<div class="hd">Вложения</div><div class="grp"><div class="blk"><b>Удалять старше</b>'
    + segs(["Никогда", "Недели", "Месяца", "Года"], "Недели") + '</div><div class="dv"></div>'
    + '<div class="blk"><b>Только файлы от</b>' + segs(["Любого", "10 МБ", "100 МБ"], "10 МБ") + '</div></div>'
    + '<div class="cap">У сообщения остаётся строка «Вложение удалено»; файл можно скачать у отправителя заново, пока он у него есть.</div>'
    + '<div class="hd">Свои правила</div><div class="grp">'
    + '<button class="r"><span class="av" style="background:#00695C;border-radius:12px;width:36px;height:36px;font-size:15px;">П</span><span class="lb">Походы</span><span class="v">Никогда</span></button>'
    + '<div class="dv" style="margin-left:64px;"></div>'
    + '<button class="r"><span class="av" style="background:#3949AB;width:36px;height:36px;font-size:15px;">А</span><span class="lb">Алиса</span><span class="v">Недели</span></button></div>'
    + '<div class="cap">Своё правило задаётся в профиле чата.</div>'))

# ---------- 3. Подтверждение ----------
confirm = page("Автоудаление: подтверждение", head("Автоудаление") + panel(
    '<div class="cap">Удаление только на этом телефоне: у собеседников ничего не меняется. «Сохранённое» и закреплённые сообщения не удаляются.</div>'
    + '<div class="hd">Сообщения</div><div class="grp"><div class="blk"><b>Удалять старше</b>'
    + segs(["Никогда", "Недели", "Месяца", "Года"], "Месяца") + '</div></div>')
    + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
    + '<div class="sht">Удалить старые сообщения?</div>'
    + '<div class="sub" style="font-size:15px;line-height:21px;padding:0 4px;">Под правило сейчас попадают 1 240 сообщений и 820 МБ вложений. Удаление — только на этом телефоне, вернуть нельзя.</div>'
    + '<button class="btn red">Удалить и включить</button><button class="btn">Отмена</button></div>')

# ---------- 4. Правило чата ----------
opts = [("Как в общих", "старше месяца", True), ("Никогда", "", False), ("Старше недели", "", False),
        ("Старше месяца", "", False), ("Старше года", "", False)]
chat_rule = page("Автоудаление: правило чата", head("Алиса") + panel(
    '<div class="grp">'
    + '<button class="r"><span class="lb"><span>Медиа и файлы</span></span><span class="v">214</span></button><div class="dv"></div>'
    + '<button class="r"><span class="lb"><span>Уведомления</span></span><span class="v">Включены</span></button><div class="dv"></div>'
    + '<button class="r"><span class="lb"><span>Автоудаление</span></span><span class="v">Как в общих</span></button></div>')
    + '<div class="dim"></div><div class="sheet"><div class="hdl"></div>'
    + '<div class="sht">Автоудаление в этом чате</div>'
    + ''.join(f'<div class="opt"><span>{t}{f"<small>{s}</small>" if s else ""}</span>{radio(on)}</div>' for t, s, on in opts)
    + '<div class="sub" style="padding:0 4px;">Только на этом телефоне. Закреплённые сообщения не удаляются.</div></div>')

rows = [[("Main.dc.html", storage, "Вход: строка в «Хранилище»"),
         ("Rules.dc.html", rules, "Правило: сообщения, вложения, свои правила чатов"),
         ("Confirm.dc.html", confirm, "Включение с удалением: число и объём до нажатия"),
         ("ChatRule.dc.html", chat_rule, "Правило чата — из его профиля")]]
row_titles = ["Автоудаление у себя"]
boards, order, notes = {}, [], {}
for ri, r in enumerate(rows):
    y = ri * 1235
    for ci, (fn, html, title) in enumerate(r):
        with open(os.path.join(P, fn), "w", encoding="utf-8") as f:
            f.write(html)
        boards[fn] = {"x": ci * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(fn)
    notes[f"t{ri + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[ri], "kind": "title1", "maxW": len(r) * 492 - 80}
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-22T13:00:00Z"}, "title": "Автоудаление Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes, "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok", len(order))
