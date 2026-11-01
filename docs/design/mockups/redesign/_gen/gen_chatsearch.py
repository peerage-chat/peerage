import json, os, sys
OUT = sys.argv[1]
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS = """body{margin:0}
a{color:#33B5E5}a:hover{color:#7FD0EE}
.ib{width:40px;height:40px;border-radius:14px;background:#16171B;border:none;display:flex;align-items:center;justify-content:center;padding:0;flex:none}
.fld{flex:1;min-width:0;height:44px;border-radius:14px;background:#16171B;display:flex;align-items:center;gap:8px;padding:0 6px 0 12px;box-sizing:border-box}
.qt{flex:1;min-width:0;color:#ECEDEF;font-size:16px;white-space:nowrap;overflow:hidden}
.ph{flex:1;color:#8E8E98;font-size:16px}
.cr{display:inline-block;width:2px;height:19px;background:#33B5E5;vertical-align:-4px;margin-left:1px}
.tok{display:flex;align-items:center;gap:4px;height:28px;padding:0 6px 0 10px;border-radius:8px;background:#13313D;color:#7FD0EE;font-size:14px;font-weight:500;flex:none}
.bin{align-self:flex-start;max-width:290px;background:#23242A;color:#E6E7EA;font-size:15px;line-height:21px;padding:7px 12px;border-radius:18px}
.bout{align-self:flex-end;max-width:290px;background:#1F4657;color:#EAF6FB;font-size:15px;line-height:21px;padding:7px 12px;border-radius:18px}
.s1{border-bottom-left-radius:6px}.s2{border-top-left-radius:6px}
.mt{font-size:11px;color:#9AA3AD;margin-left:8px;white-space:nowrap}
.mto{font-size:11px;color:#9FC6D6;margin-left:8px;white-space:nowrap}
.lbl{align-self:center;font-size:12px;color:#B8B8C0;background:rgba(255,255,255,0.06);padding:3px 10px;border-radius:10px;margin:4px 0}
.q{background:rgba(255,255,255,0.1);border-left:3px solid #7FD0EE;border-radius:10px;padding:4px 8px;margin-bottom:5px;font-size:13px;line-height:17px}
.hl{background:rgba(51,181,229,0.30);color:#FFFFFF;border-radius:4px;padding:0 2px}
.hlc{background:#33B5E5;color:#04222E;border-radius:4px;padding:0 2px}
.grp{background:#1C1D22;border-radius:18px;display:flex;flex-direction:column;overflow:hidden;flex:none}
.r{display:flex;align-items:center;gap:14px;min-height:52px;padding:9px 14px;background:none;border:none;width:100%;box-sizing:border-box;font-family:inherit;color:#ECEDEF;font-size:16px;text-align:left}
.lb{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}
.sub{font-size:13px;line-height:18px;color:#8E8E98}
.v{color:#8E8E98;font-size:14px;flex:none}
.dv{height:1px;background:#26272D;margin-left:48px}
.hd{font-size:13px;font-weight:500;color:#8E8E98;padding:0 6px;margin-bottom:-6px}
.chip{display:flex;align-items:center;gap:6px;height:34px;padding:0 10px 0 14px;border-radius:17px;background:#1C1D22;border:none;color:#C4C7CC;font-family:inherit;font-size:14px;flex:none}
.chip.on{background:#13313D;color:#7FD0EE}
.pill{display:flex;align-items:center;gap:8px;height:40px;padding:0 14px;border-radius:14px;background:#1C1D22;border:none;color:#ECEDEF;font-family:inherit;font-size:14px;font-weight:500;flex:none}
.bar{display:flex;align-items:center;gap:8px;padding:8px 12px 16px;border-top:1px solid #222328}
.cnt{flex:1;font-size:15px;color:#ECEDEF;font-variant-numeric:tabular-nums}
.av{width:36px;height:36px;border-radius:50%;color:#FFFFFF;font-size:15px;font-weight:500;display:flex;align-items:center;justify-content:center;flex:none}
.hit{display:flex;gap:12px;padding:10px 14px;align-items:flex-start}
.hn{display:flex;align-items:baseline;gap:8px}
.hn b{flex:1;font-size:15px;font-weight:500;color:#ECEDEF}
.hn span{font-size:12px;color:#8E8E98;font-variant-numeric:tabular-nums}
.hs{font-size:14px;line-height:19px;color:#B8B8C0;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;margin-top:2px}
.hdv{height:1px;background:#26272D;margin-left:62px}
.cal{display:grid;grid-template-columns:repeat(7,1fr);row-gap:2px;text-align:center}
.wd{font-size:12px;color:#8E8E98;height:28px;line-height:28px}
.d{height:44px;display:flex;align-items:center;justify-content:center}
.d i{font-style:normal;width:40px;height:40px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:15px;color:#ECEDEF;font-variant-numeric:tabular-nums}
.d.no i{color:#4A4B52}
.d.sel i{background:#33B5E5;color:#04222E;font-weight:500}
"""


def ic(path, size=20, color="#E6E7EA", extra=""):
    return (f'<svg width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{color}" '
            f'stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"{extra}>{path}</svg>')


NOFLEX = ' style="flex:none;"'
BACK = '<button class="ib" style="background: none;" aria-label="Закрыть поиск">' + ic('<path d="M15 5l-7 7 7 7"></path>', 22) + '</button>'
SEARCH = ic('<circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path>', 18, "#9AA3AD", NOFLEX)
CLEAR = '<button class="ib" style="width:32px;height:32px;background:none;" aria-label="Очистить">' + ic('<path d="M6 6l12 12M18 6L6 18"></path>', 18, "#9AA3AD") + '</button>'
CAL_P = '<rect x="4" y="5" width="16" height="15" rx="2"></rect><path d="M4 10h16M9 3v4M15 3v4"></path>'
CALBTN = '<button class="ib" aria-label="Перейти к дате">' + ic(CAL_P) + '</button>'
CALBTN_ON = '<button class="ib" style="background:#13313D;" aria-label="Перейти к дате">' + ic(CAL_P, 20, "#7FD0EE") + '</button>'
CHEV = ic('<path d="M9 6l6 6-6 6"></path>', 18, "#8E8E98", NOFLEX)
DOWN = ic('<path d="M7 10l5 5 5-5"></path>', 16, "currentColor")
XS = ic('<path d="M7 7l10 10M17 7L7 17"></path>', 14, "#7FD0EE")
TICKS = ('<svg width="16" height="10" viewBox="0 0 18 12" fill="none" stroke="#9AD7EE" stroke-width="1.8" '
         'stroke-linecap="round" stroke-linejoin="round" aria-label="Доставлено" role="img"><path d="M1.5 6.5l3.2 3.2L11 2.5"></path><path d="M8.2 9.2l.5.5L15 2.5"></path></svg>')
LIST_P = '<path d="M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01"></path>'
FEED_P = '<path d="M4 6h10M10 12h10M4 18h10"></path>'


def header(field, right=CALBTN):
    return ('<div style="display: flex; align-items: center; gap: 8px; padding: 14px 12px 14px 8px;">'
            + BACK + f'<div class="fld">{SEARCH}{field}</div>' + right + '</div>')


def page(title, body):
    return f"""<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>{title}</title>
<script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Golos+Text:wght@400;500;700&amp;display=swap">
<style>
{CSS}</style>
</helmet>
<div style="width: 412px; height: 892px; box-sizing: border-box; background: #08090B; font-family: 'Golos Text', system-ui, sans-serif; display: flex; flex-direction: column; overflow: hidden; position: relative;">
{body}
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{{"$preview":{{"width":412,"height":892}}}}'>
class Component extends DCLogic {{
renderVals() {{
return {{}};
}}
}}
</script>
</body>
</html>
"""


PANEL = '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; overflow: hidden;">'
PANEL_PAD = '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; overflow: hidden; gap: 14px; padding: 10px;">'


def row(icon, label, value="", sub=""):
    lb = f'<span class="lb"><span>{label}</span>' + (f'<span class="sub">{sub}</span>' if sub else '') + '</span>'
    v = f'<span class="v">{value}</span>' if value else ''
    return f'<button class="r">{ic(icon, 20, "#9AA3AD", NOFLEX)}{lb}{v}{CHEV}</button>'


DV = '<div class="dv"></div>'

# ---------- Начало ----------
USER_P = '<circle cx="12" cy="8" r="3.5"></circle><path d="M5 20a7 7 0 0 1 14 0"></path>'
PHOTO_P = '<rect x="4" y="5" width="16" height="14" rx="2"></rect><circle cx="9" cy="10" r="1.5"></circle><path d="M20 16l-5-5-9 8"></path>'
FILE_P = '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"></path><path d="M14 3v5h5"></path>'
LINK_P = '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"></path><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"></path>'
MIC_P = '<rect x="9" y="3" width="6" height="11" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0M12 18v3"></path>'

start = page("Поиск в чате: начало", header('<span class="ph">Поиск в чате<span class="cr"></span></span>', '') + PANEL_PAD + f"""
<div class="hd" style="margin-top:4px;">Уточнить</div>
<div class="grp">
{row(USER_P, "От участника", sub="Только сообщения одного человека")}
{DV}
{row(CAL_P, "Перейти к дате", sub="Календарь переписки")}
</div>
<div class="hd">Медиа и файлы</div>
<div class="grp">
{row(PHOTO_P, "Фото и видео", "248")}
{DV}
{row(FILE_P, "Файлы", "31")}
{DV}
{row(LINK_P, "Ссылки", "57")}
{DV}
{row(MIC_P, "Голосовые", "12")}
</div>
<div class="sub" style="padding: 0 6px;">Поиск идёт по тексту сообщений, которые хранятся на этом устройстве. Запрос никуда не отправляется.</div>
</div>""")


# ---------- А: в ленте ----------
def bar(count, btn_icon, btn_label, arrows=True):
    a = ''
    if arrows:
        a = ('<button class="ib" style="background:#1C1D22;" aria-label="Предыдущее совпадение">' + ic('<path d="M6 15l6-6 6 6"></path>') + '</button>'
             '<button class="ib" style="background:#1C1D22;" aria-label="Следующее совпадение">' + ic('<path d="M6 9l6 6 6-6"></path>') + '</button>')
    return (f'<div class="bar"><span class="cnt">{count}</span>'
            f'<button class="pill">{ic(btn_icon, 18, "#E6E7EA")}{btn_label}</button>{a}</div>')


feed = page("Поиск в чате: в ленте", header('<span class="qt">книг<span class="cr"></span></span>' + CLEAR) + PANEL + f"""
<div style="flex: 1; display: flex; flex-direction: column; justify-content: flex-end; gap: 3px; padding: 12px 14px;">
<div class="lbl">12 октября</div>
<div class="bin s1">Дочитала ту <span class="hl">книгу</span>, что ты советовал<span class="mt">18:02</span></div>
<div class="bin s2">Конец так себе, но середина — огонь<span class="mt">18:03</span></div>
<div style="height: 6px;"></div>
<div class="bout">а я говорил<span class="mto">18:05 {TICKS}</span></div>
<div class="lbl">Сегодня</div>
<div class="bin s1">Ты сегодня дома?<span class="mt">14:18</span></div>
<div class="bin s2">Хочу занести <span class="hlc">книгу</span><span class="mt">14:18</span></div>
<div style="height: 6px;"></div>
<div class="bout"><div class="q"><div style="color:#9AD7EE;font-weight:500;">Алиса</div>Хочу занести книгу</div>да, после семи<span class="mto">14:19 {TICKS}</span></div>
<div class="bout">и захвати вторую <span class="hl">книжку</span>, если не жалко<span class="mto">14:20 {TICKS}</span></div>
</div>
{bar("2 из 12", LIST_P, "Списком")}
</div>""")


# ---------- Б: списком ----------
def hit(av, color, name, time, snip, last=False):
    return (f'<div class="hit"><div class="av" style="background:{color};">{av}</div>'
            f'<div style="flex:1;min-width:0;"><div class="hn"><b>{name}</b><span>{time}</span></div>'
            f'<div class="hs">{snip}</div></div></div>' + ('' if last else '<div class="hdv"></div>'))


lst = page("Поиск в чате: списком", header('<span class="qt">ключ<span class="cr"></span></span>' + CLEAR) + PANEL + f"""
<div style="display: flex; gap: 8px; padding: 12px 10px 0;">
<button class="chip">От участника{DOWN}</button>
<button class="chip">Дата{DOWN}</button>
</div>
<div style="flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 14px; padding: 14px 10px 10px; overflow: hidden;">
<div class="hd">Сегодня</div>
<div class="grp">
{hit("Б", "#00897B", "Борис", "12:40", '<span class="hl">Ключ</span> оставлю у соседки, кв. 14 — она до вечера дома')}
{hit("Я", "#2B5F73", "Вы", "12:44", 'А второй <span class="hl">ключ</span> от сарая у кого?', True)}
</div>
<div class="hd">Вчера</div>
<div class="grp">
{hit("М", "#8E24AA", "Мария", "21:15", 'Запасной <span class="hl">ключ</span> под горшком у крыльца, справа. Только никому не говорите, это последний', True)}
</div>
<div class="hd">3 октября</div>
<div class="grp">
{hit("Б", "#00897B", "Борис", "10:02", 'Поменял замок, новые <span class="hl">ключи</span> раздам в субботу')}
{hit("Я", "#2B5F73", "Вы", "10:05", 'Мне один <span class="hl">ключ</span>, второй Маше', True)}
</div>
</div>
{bar("5 сообщений", FEED_P, "В ленте", arrows=False)}
</div>""")

# ---------- Перейти к дате ----------
msg_days = {1, 2, 3, 5, 8, 9, 10, 12, 13, 14, 17, 20, 21, 22, 24, 27, 28, 29, 30, 31}
cells = ''.join(f'<div class="wd">{w}</div>' for w in ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"])
cells += '<div></div>' * 3  # 1 октября 2026 — четверг
for d in range(1, 32):
    cls = "d" + ("" if d in msg_days else " no") + (" sel" if d == 12 else "")
    cells += f'<div class="{cls}"><i>{d}</i></div>'

dim_feed = """<div style="flex: 1; display: flex; flex-direction: column; justify-content: flex-end; gap: 3px; padding: 12px 14px;">
<div class="lbl">Сегодня</div>
<div class="bin s1">Ты сегодня дома?<span class="mt">14:18</span></div>
<div class="bin s2">Хочу занести книгу<span class="mt">14:18</span></div>
</div>"""
date = page("Поиск в чате: перейти к дате", header('<span class="ph">Поиск в чате</span>', CALBTN_ON) + PANEL + dim_feed + f"""
</div>
<div style="position: absolute; inset: 0; background: rgba(0,0,0,0.55);"></div>
<div style="position: absolute; left: 0; right: 0; bottom: 0; background: #1C1D22; border-radius: 28px 28px 0 0; padding: 10px 16px 20px; display: flex; flex-direction: column; gap: 10px;">
<div style="align-self: center; width: 36px; height: 4px; border-radius: 2px; background: #3A3B42;"></div>
<div style="font-size: 20px; font-weight: 700; color: #ECEDEF; padding: 6px 4px 0;">Перейти к дате</div>
<div style="display: flex; align-items: center; gap: 8px;">
<button class="ib" style="background:none;" aria-label="Предыдущий месяц">{ic('<path d="M15 5l-7 7 7 7"></path>')}</button>
<button style="flex: 1; background: none; border: none; color: #ECEDEF; font-family: inherit; font-size: 16px; font-weight: 500; display: flex; align-items: center; justify-content: center; gap: 6px;">Октябрь 2026{DOWN}</button>
<button class="ib" style="background:none;" aria-label="Следующий месяц">{ic('<path d="M9 5l7 7-7 7"></path>')}</button>
</div>
<div class="cal">{cells}</div>
<div class="sub" style="padding: 0 4px;">Тусклые дни — без сообщений. Нажатие на день открывает его первое сообщение.</div>
<div style="display: flex; gap: 8px; padding-top: 4px;">
<button class="pill" style="flex:1;justify-content:center;background:#26272D;">К первому сообщению</button>
<button class="pill" style="flex:1;justify-content:center;background:#26272D;">К последнему</button>
</div>
</div>""")

# ---------- Ничего не найдено ----------
tok = '<span class="tok">Борис' + XS + '</span>'
empty = page("Поиск в чате: ничего не найдено", header(tok + '<span class="qt">сарай<span class="cr"></span></span>' + CLEAR) + PANEL + f"""
<div style="display: flex; gap: 8px; padding: 12px 10px 0;">
<button class="chip on">От: Борис{XS}</button>
<button class="chip">Дата{DOWN}</button>
</div>
<div style="flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; padding: 0 40px 80px; text-align: center;">
{ic('<circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path>', 40, "#4A4B52")}
<div style="font-size: 16px; font-weight: 500; color: #ECEDEF; margin-top: 6px;">Ничего не найдено</div>
<div class="sub">Среди сообщений Бориса совпадений нет</div>
<button class="pill" style="margin-top: 10px;">Искать у всех участников</button>
</div>
</div>""")

files = {
    "Main.dc.html": (start, "Начало: пустой запрос — уточнения и разделы медиа"),
    "Feed.dc.html": (feed, "А · В ленте: подсветка, счётчик, ↑ ↓"),
    "List.dc.html": (lst, "Б · Списком: находки по дням, фильтры"),
    "Date.dc.html": (date, "Перейти к дате: календарь, пустые дни приглушены"),
    "Empty.dc.html": (empty, "Ничего не найдено — с фильтром по участнику"),
}
boards, order = {}, []
for i, (name, (html, title)) in enumerate(files.items()):
    with open(os.path.join(P, name), "w", encoding="utf-8") as f:
        f.write(html)
    boards[name] = {"x": i * 492, "y": 0, "w": 412, "h": 892, "title": title}
    order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T19:00:00Z"}, "title": "Поиск в чате Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order,
          "notes": {"t1": {"x": 0, "y": -300, "text": "Поиск по истории чата", "kind": "title1", "maxW": 2380}},
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
