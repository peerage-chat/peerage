import json, os, sys
OUT = sys.argv[1]
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS = """body{margin:0}
a{color:#33B5E5}a:hover{color:#7FD0EE}
.row{display:flex;gap:12px;align-items:center;padding:12px 16px;min-height:72px;box-sizing:border-box;width:100%;background:none;border:none;font-family:inherit;text-align:left}
.av{width:48px;height:48px;flex:none;display:flex;align-items:center;justify-content:center;color:#fff;font-weight:500;font-size:18px;position:relative}
.p{border-radius:50%}.g{border-radius:14px}
.on{position:absolute;right:-1px;bottom:-1px;width:12px;height:12px;border-radius:50%;background:#8BC34A;border:2.5px solid #16171B}
.mid{flex:1;min-width:0;display:flex;flex-direction:column;gap:3px}
.nm{font-size:16px;font-weight:500;color:#ECEDEF;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;display:flex;align-items:center;gap:6px}
.pv{font-size:14px;color:#8E8E98;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.pv b{font-weight:500;color:#B8B8C0}
.side{display:flex;flex-direction:column;align-items:flex-end;justify-content:space-between;min-height:44px;flex:none;gap:4px}
.tm{font-size:12px;color:#7A7A84}
.bd{min-width:22px;height:22px;border-radius:11px;background:#33B5E5;color:#04222E;font-size:12px;font-weight:700;display:flex;align-items:center;justify-content:center;padding:0 7px;box-sizing:border-box}
.bm{background:#3A3A42;color:#C8C8D0}
.dv{height:1px;background:#222328;margin-left:76px}
.tab{min-height:40px;padding:0 4px;background:none;border:none;border-bottom:2px solid transparent;color:#8E8E98;font-size:15px;font-weight:500;font-family:inherit;display:flex;align-items:center;gap:6px}
.tab.sel{color:#ECEDEF;border-bottom-color:#33B5E5}
.tab .bd{min-width:20px;height:20px;font-size:11px;padding:0 6px}
.ib{width:40px;height:40px;border-radius:14px;background:#16171B;border:none;display:flex;align-items:center;justify-content:center;padding:0;flex:none}
.strip{display:flex;align-items:center;gap:12px;margin:8px 10px 4px;padding:12px 14px;background:#13313D;border:none;border-radius:18px;font-family:inherit;text-align:left;width:calc(100% - 20px);box-sizing:border-box}
.tag{font-size:11px;font-weight:500;color:#7FD0EE;background:#13313D;border-radius:6px;padding:2px 6px;flex:none}
.grp{background:#1C1D22;border-radius:18px;display:flex;flex-direction:column;overflow:hidden;flex:none}
.rq{display:flex;gap:12px;align-items:center;padding:12px 12px 12px 14px;min-height:72px;box-sizing:border-box}
.hd{font-size:13px;font-weight:500;color:#8E8E98;padding:0 6px;margin-bottom:-6px}
.sub{font-size:13px;line-height:18px;color:#8E8E98}
.ok{min-height:36px;padding:0 14px;border-radius:12px;border:none;background:#33B5E5;color:#04222E;font-family:inherit;font-size:14px;font-weight:500;flex:none}
.no{width:36px;height:36px;border-radius:12px;border:none;background:#26272D;display:flex;align-items:center;justify-content:center;flex:none;padding:0}
.rdv{height:1px;background:#26272D;margin-left:74px}
"""


def ic(path, size=20, color="#E6E7EA", extra=""):
    return (f'<svg width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{color}" '
            f'stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"{extra}>{path}</svg>')


NF = ' style="flex:none;"'
SEARCH = '<circle cx="11" cy="11" r="7"></circle><path d="M20 20l-4-4"></path>'
PLUSP = '<path d="M12 5v14M5 12h14"></path>'
CHEV = '<path d="M9 6l6 6-6 6"></path>'
CLOSE = '<path d="M6 6l12 12M18 6L6 18"></path>'
EDIT = '<path d="M4 20h4L19 9l-4-4L4 16z"></path><path d="M13.5 6.5l4 4"></path>'
WARN_T = '<path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17h.01"></path>'
USERPLUS = '<circle cx="10" cy="8" r="4"></circle><path d="M3 20a7 7 0 0 1 12.5-4.3M19 14v6M16 17h6"></path>'
TICKS = ('<svg width="18" height="12" viewBox="0 0 18 12" fill="none" stroke="#33B5E5" stroke-width="1.8" stroke-linecap="round" '
         'stroke-linejoin="round" aria-label="Доставлено" role="img"><path d="M1.5 6.5l3.2 3.2L11 2.5"></path><path d="M8.2 9.2l.5.5L15 2.5"></path></svg>')
WARN_I = f'<span role="img" aria-label="Имя совпадает с контактом" style="display:inline-flex;">{ic(WARN_T, 15, "#FFB74D")}</span>'


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
<div style="width: 412px; height: 892px; box-sizing: border-box; background: #08090B; font-family: 'Golos Text', system-ui, sans-serif; display: flex; flex-direction: column; position: relative; overflow: hidden;">
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


HOME_HEAD = ('<div style="display: flex; align-items: center; gap: 12px; padding: 18px 16px 18px;">'
             '<button style="width: 40px; height: 40px; border-radius: 50%; background: #5D2A2A; color: #FFFFFF; font-size: 16px; font-weight: 500; border: none; font-family: inherit;" aria-label="Профиль">А</button>'
             '<div style="flex: 1; display: flex; flex-direction: column;"><div style="font-size: 22px; font-weight: 700; color: #ECEDEF;">Чаты</div>'
             '<div style="font-size: 12px; color: #8BC34A;">В сети · 8 узлов</div></div>'
             f'<button class="ib" aria-label="Поиск">{ic(SEARCH)}</button></div>')
FAB = (f'<button style="position: absolute; right: 16px; bottom: 24px; width: 60px; height: 60px; border-radius: 18px; background: #33B5E5; border: none; display: flex; align-items: center; justify-content: center;" aria-label="Новый чат">{ic(EDIT, 24, "#04222E")}</button>')


def tabs(extra=""):
    return ('<div style="display: flex; gap: 20px; padding: 0 20px; border-bottom: 1px solid #222328;">'
            '<button class="tab sel">Все</button><button class="tab">Личные</button><button class="tab">Группы</button>' + extra
            + f'<button class="tab" aria-label="Новая папка">{ic(PLUSP, 18, "currentColor")}</button></div>')


def chat(av, color, shape, name, pv, tm, side, online=False):
    on = '<i class="on"></i>' if online else ''
    return (f'<div class="row"><div class="av {shape}" style="background:{color}">{av}{on}</div>'
            f'<div class="mid"><div class="nm">{name}</div><div class="pv">{pv}</div></div>'
            f'<div class="side"><div class="tm">{tm}</div>{side}</div></div>')


DV = '<div class="dv"></div>'
CHATS = [
    chat("П", "#00695C", "g", "Походы", '<b>Боб:</b> <span style="color:#33B5E5">@fix</span> завтра в 8 у станции?', "14:02", '<div class="bd">@</div>'),
    chat("К", "#AD4E1E", "p", "Карина", "Вы: Фото", "13:40", TICKS),
    chat("Р", "#4E5B8A", "g", "Работа", "<b>Лена:</b> созвон переносим на 16", "12:10", '<div class="bd bm">12</div>'),
    chat("В", "#6D4C41", "p", "Влад", "Пропущенный звонок", "вчера", "", True),
    chat("Б", "#2E6B3A", "p", "Боб", "Вы: ок, до связи", "вт", TICKS),
]


def chat_list(n=5):
    return DV.join(CHATS[:n])


PANEL = '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; padding-top: 6px; overflow: hidden;">'

STRIP = ('<button class="strip">' + ic(USERPLUS, 20, "#7FD0EE", NF)
         + '<span style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px;"><span style="font-size: 14px; font-weight: 500; color: #DFF3FB;">Запросы и приглашения · 3</span>'
         + '<span style="font-size: 12px; color: #9FC6D6; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">Алиса, Лиза · группа «Дача 2026»</span></span>'
         + ic(CHEV, 18, "#9FC6D6", NF) + '</button>')

# ---------- А · полоса над списком ----------
a = page("Запросы: полоса на главной", HOME_HEAD + PANEL + tabs() + STRIP + chat_list() + '</div>' + FAB)

# ---------- Б · строки в списке чатов ----------
req_rows = (
    f'<div class="row" style="background:#1A1B20;"><div class="av p" style="background:#455A64">А</div>'
    f'<div class="mid"><div class="nm">Алиса {WARN_I}<span class="tag">Запрос</span></div><div class="pv">Привет, это я, телефон новый. Скинешь…</div></div>'
    f'<div class="side"><div class="tm">14:31</div></div></div>' + DV
    + f'<div class="row" style="background:#1A1B20;"><div class="av g" style="background:#5D4037">Д</div>'
    f'<div class="mid"><div class="nm">Дача 2026<span class="tag">Приглашение</span></div><div class="pv">от Бориса · 4 участника</div></div>'
    f'<div class="side"><div class="tm">12:05</div></div></div>' + DV
    + f'<div class="row" style="background:#1A1B20;"><div class="av p" style="background:#00838F">Л</div>'
    f'<div class="mid"><div class="nm">Лиза<span class="tag">Запрос</span></div><div class="pv">Без сообщения</div></div>'
    f'<div class="side"><div class="tm">вчера</div></div></div>' + DV)
b = page("Запросы: строки в списке", HOME_HEAD + PANEL + tabs() + '<div style="height:6px;"></div>' + req_rows + chat_list(3) + '</div>' + FAB)

# ---------- В · вкладка ----------
c = page("Запросы: вкладка", HOME_HEAD + PANEL
         + tabs('<button class="tab">Запросы<span class="bd">3</span></button>')
         + '<div style="height:6px;"></div>' + chat_list() + '</div>' + FAB)


# ---------- Экран «Запросы и приглашения» ----------
def rq(av, color, shape, name, sub, accept, warn=False):
    w = f' {WARN_I}' if warn else ''
    return (f'<div class="rq"><div class="av {shape}" style="background:{color}">{av}</div>'
            f'<div class="mid"><div class="nm">{name}{w}</div><div class="pv">{sub}</div></div>'
            f'<button class="ok">{accept}</button><button class="no" aria-label="Отклонить">{ic(CLOSE, 18, "#B8B8C0")}</button></div>')


screen = page("Запросы и приглашения", '<div style="display: flex; align-items: center; gap: 10px; padding: 14px 12px 14px 8px;">'
              + f'<button class="ib" style="background: none;" aria-label="Назад">{ic("<path d=" + chr(34) + "M15 5l-7 7 7 7" + chr(34) + "></path>", 22)}</button>'
              + '<div style="flex: 1; font-size: 22px; font-weight: 700; color: #ECEDEF;">Запросы и приглашения</div></div>'
              + '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; gap: 14px; padding: 10px; overflow: hidden;">'
              + '<div class="hd" style="margin-top:4px;">В контакты · 2</div><div class="grp">'
              + rq("А", "#455A64", "p", "Алиса", "Имя совпадает с контактом, ключ другой", "Открыть", warn=True)
              + '<div class="rdv"></div>'
              + rq("Л", "#00838F", "p", "Лиза", "Без сообщения · вчера", "Добавить")
              + '</div><div class="hd">В группы · 1</div><div class="grp">'
              + rq("Д", "#5D4037", "g", "Дача 2026", "от Бориса · 4 участника", "Вступить")
              + '</div>'
              + '<div class="sub" style="padding:0 6px;">Нажатие на строку открывает карточку: ключ, первое сообщение, участники. Запрос с совпадающим именем принимается только из карточки.</div>'
              + '</div>')

# ---------- Контакты с той же полосой ----------
contacts = page("Контакты с полосой", '<div style="display: flex; align-items: center; gap: 10px; padding: 14px 12px 14px 8px;">'
                + f'<button class="ib" style="background: none;" aria-label="Назад">{ic("<path d=" + chr(34) + "M15 5l-7 7 7 7" + chr(34) + "></path>", 22)}</button>'
                + '<div style="flex: 1; font-size: 22px; font-weight: 700; color: #ECEDEF;">Контакты</div>'
                + f'<button class="ib" aria-label="Поиск">{ic(SEARCH)}</button>'
                + f'<button class="ib" style="background: #33B5E5;" aria-label="Добавить контакт">{ic(PLUSP, 20, "#04222E")}</button></div>'
                + PANEL + STRIP
                + ''.join(chat(a_, c_, "p", n_, s_, "", "", o_).replace('<div class="side"><div class="tm"></div></div>', '')
                          for a_, c_, n_, s_, o_ in [("А", "#3949AB", "Алиса", '<span style="color:#8BC34A;">в сети</span>', True),
                                                     ("Б", "#2E6B3A", "Боб", '<span style="color:#8BC34A;">в сети</span>', True),
                                                     ("В", "#6D4C41", "Влад", "не в сети", False),
                                                     ("К", "#AD4E1E", "Карина", "не в сети", False),
                                                     ("Л", "#7B3F7A", "Лена", "не в сети", False)])
                + '</div>')

items = [("Main.dc.html", a, "А · Полоса над списком (как в контактах)"),
         ("Rows.dc.html", b, "Б · Строки прямо в списке чатов, с меткой"),
         ("Tab.dc.html", c, "В · Вкладка «Запросы» (видна, пока есть)"),
         ("Screen.dc.html", screen, "Экран «Запросы и приглашения» (из полосы А)"),
         ("Contacts.dc.html", contacts, "Контакты: та же полоса")]
boards, order = {}, []
for i, (name, html, title) in enumerate(items):
    with open(os.path.join(P, name), "w", encoding="utf-8") as f:
        f.write(html)
    boards[name] = {"x": i * 492, "y": 0, "w": 412, "h": 892, "title": title}
    order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T21:30:00Z"}, "title": "Запросы и приглашения Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order,
          "notes": {"t1": {"x": 0, "y": -300, "text": "Где живут запросы и приглашения", "kind": "title1", "maxW": 2380}},
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
