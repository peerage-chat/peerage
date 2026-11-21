import json, os, sys
OUT = sys.argv[1]
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS = """body{margin:0}
a{color:#33B5E5}a:hover{color:#7FD0EE}
.ib{width:40px;height:40px;border-radius:14px;background:#16171B;border:none;display:flex;align-items:center;justify-content:center;padding:0;flex:none}
.bin{align-self:flex-start;max-width:290px;background:#23242A;color:#E6E7EA;font-size:15px;line-height:21px;padding:7px 12px;border-radius:18px}
.bout{align-self:flex-end;max-width:290px;background:#1F4657;color:#EAF6FB;font-size:15px;line-height:21px;padding:7px 12px;border-radius:18px}
.s1{border-bottom-left-radius:6px}.s2{border-top-left-radius:6px}
.o1{border-bottom-right-radius:6px}.o2{border-top-right-radius:6px}
.mt{font-size:11px;color:#9AA3AD;margin-left:8px;white-space:nowrap}
.mto{font-size:11px;color:#9FC6D6;margin-left:8px;white-space:nowrap}
.lbl{align-self:center;font-size:12px;color:#B8B8C0;background:rgba(255,255,255,0.06);padding:3px 10px;border-radius:10px;margin:4px 0}
.q{background:rgba(255,255,255,0.1);border-left:3px solid #7FD0EE;border-radius:10px;padding:4px 8px;margin-bottom:5px;font-size:13px;line-height:17px}
.sel{background:rgba(51,181,229,0.16);margin:0 -14px;padding:2px 14px;display:flex;flex-direction:column}
.row{margin:0 -14px;padding:2px 14px;display:flex;flex-direction:column}
.ck{position:absolute;left:12px;top:50%;margin-top:-11px;width:22px;height:22px;border-radius:50%;box-sizing:border-box;border:1.5px solid #6B6C74}
.ck.on{border:none;background:#33B5E5;display:flex;align-items:center;justify-content:center}
.cmp{display:flex;align-items:flex-end;gap:8px;padding:8px 12px 16px}
.fld{flex:1;min-height:46px;border-radius:16px;background:#1C1D22;display:flex;align-items:center;gap:10px;padding:0 12px;box-sizing:border-box}
.ph{flex:1;color:#8E8E98;font-size:15px}
.tx{flex:1;color:#ECEDEF;font-size:15px;line-height:21px;padding:12px 0}
.cr{display:inline-block;width:2px;height:18px;background:#33B5E5;vertical-align:-3px;margin-left:1px}
.snd{width:46px;height:46px;border-radius:16px;background:#33B5E5;border:none;display:flex;align-items:center;justify-content:center;flex:none}
.strip{display:flex;align-items:center;gap:10px;margin:0 12px;padding:8px 6px 8px 12px;background:#1C1D22;border-radius:16px 16px 6px 6px}
.strip + .cmp{padding-top:2px}
.sti{flex:1;min-width:0}
.stt{font-size:13px;font-weight:500;color:#33B5E5}
.sts{font-size:13px;color:#B8B8C0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.menu{background:#1C1D22;border-radius:18px;overflow:hidden;display:flex;flex-direction:column;box-shadow:0 12px 32px rgba(0,0,0,0.5),0 0 0 1px rgba(255,255,255,0.04)}
.mi{display:flex;align-items:center;gap:14px;min-height:48px;padding:0 16px;background:none;border:none;font-family:inherit;color:#ECEDEF;font-size:15px;text-align:left;width:100%;box-sizing:border-box}
.mi.red{color:#FF6B6B}
.mdv{height:1px;background:#26272D}
.sheet{position:absolute;left:0;right:0;bottom:0;background:#1C1D22;border-radius:28px 28px 0 0;padding:10px 16px 20px;display:flex;flex-direction:column;gap:12px}
.hdl{align-self:center;width:36px;height:4px;border-radius:2px;background:#3A3B42}
.sht{font-size:20px;font-weight:700;color:#ECEDEF;padding:6px 4px 0}
.sub{font-size:13px;line-height:18px;color:#8E8E98}
.btn{display:flex;align-items:center;justify-content:center;gap:8px;min-height:48px;border-radius:14px;border:none;font-family:inherit;font-size:15px;font-weight:500;background:#26272D;color:#ECEDEF;width:100%}
.btn.red{background:#3A1E22;color:#FF8A8A}
.btn.pri{background:#33B5E5;color:#04222E}
.dim{position:absolute;inset:0;background:rgba(0,0,0,0.6)}
.tile{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;min-height:72px;border-radius:14px;background:#26272D;border:none;font-family:inherit;color:#ECEDEF;font-size:13px;flex:1}
.av{width:40px;height:40px;border-radius:50%;color:#FFFFFF;font-size:16px;font-weight:500;display:flex;align-items:center;justify-content:center;flex:none}
.lr{display:flex;align-items:center;gap:12px;min-height:56px;padding:0 4px}
.lr b{flex:1;font-size:15px;font-weight:500;color:#ECEDEF}
.rc{width:22px;height:22px;border-radius:50%;box-sizing:border-box;border:1.5px solid #6B6C74;flex:none}
.rc.on{border:none;background:#33B5E5;display:flex;align-items:center;justify-content:center}
.tg{width:44px;height:26px;border-radius:13px;background:#3A3B42;position:relative;flex:none}
.tg i{position:absolute;left:3px;top:3px;width:20px;height:20px;border-radius:50%;background:#8E8E98}
.ph3{aspect-ratio:1;border-radius:8px;position:relative;box-shadow:inset 0 0 0 1px rgba(255,255,255,0.1)}
.num{position:absolute;right:6px;top:6px;width:22px;height:22px;border-radius:50%;background:#33B5E5;color:#04222E;font-size:12px;font-weight:700;display:flex;align-items:center;justify-content:center}
.rx{width:40px;height:40px;border-radius:50%;border:none;background:none;font-size:24px;line-height:1;display:flex;align-items:center;justify-content:center;padding:0;flex:none}
.chips{display:flex;flex-wrap:wrap;gap:4px;margin-top:3px}
.rc2{display:flex;align-items:center;gap:4px;min-height:28px;padding:0 9px;border-radius:14px;border:none;background:#23242A;color:#C4C7CC;font-family:inherit;font-size:13px;font-variant-numeric:tabular-nums}
.rc2 span{font-size:15px}
.rc2.me{background:#13313D;color:#7FD0EE;box-shadow:inset 0 0 0 1px #33B5E5}
.tabs{display:flex;gap:6px;overflow:hidden}
.tb{display:flex;align-items:center;gap:6px;min-height:36px;padding:0 12px;border-radius:18px;border:none;background:#26272D;color:#C4C7CC;font-family:inherit;font-size:14px}
.tb.on{background:#13313D;color:#7FD0EE}
.ring{position:absolute;right:6px;top:6px;width:22px;height:22px;border-radius:50%;box-sizing:border-box;border:1.5px solid rgba(255,255,255,0.85)}
"""


def ic(path, size=20, color="#E6E7EA", extra=""):
    return (f'<svg width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{color}" '
            f'stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"{extra}>{path}</svg>')


NF = ' style="flex:none;"'
REPLY = '<path d="M10 8L5 12l5 4"></path><path d="M5 12h9a5 5 0 0 1 5 5v1"></path>'
FWD = '<path d="M14 8l5 4-5 4"></path><path d="M19 12h-9a5 5 0 0 0-5 5v1"></path>'
COPY = '<rect x="8" y="8" width="12" height="12" rx="2"></rect><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"></path>'
PIN = '<path d="M15 4.5l-4 4-4 1.5-1.5 1.5 7 7 1.5-1.5 1.5-4 4-4"></path><path d="M9 15l-4.5 4.5M14.5 4l5.5 5.5"></path>'
EDIT = '<path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z"></path><path d="M13.5 6.5l4 4"></path>'
SAVE = '<path d="M7 4h10a1 1 0 0 1 1 1v15l-6-4-6 4V5a1 1 0 0 1 1-1z"></path>'
SELECT = '<circle cx="12" cy="12" r="9"></circle><path d="M8.5 12l2.5 2.5 4.5-5"></path>'
TRASH = '<path d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3"></path>'
CLOSE = '<path d="M6 6l12 12M18 6L6 18"></path>'
LOCK = '<rect x="6" y="11" width="12" height="9" rx="2"></rect><path d="M9 11V8a3 3 0 0 1 6 0v3"></path>'
CAMERA = '<path d="M5 7h3l2-2h4l2 2h3a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z"></path><circle cx="12" cy="13" r="3.5"></circle>'
FILE = '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"></path><path d="M14 3v5h5"></path>'
MUSIC = '<circle cx="7" cy="17" r="2.5"></circle><circle cx="17" cy="15" r="2.5"></circle><path d="M9.5 17V6l10-2v11"></path>'
SEND = '<path d="M4 12l16-8-6 16-3-7z"></path><path d="M11 13l9-9"></path>'
CHECK = '<path d="M5 12l5 5L19 7"></path>'
MIC = '<rect x="9" y="3" width="6" height="11" rx="3"></rect><path d="M5 11a7 7 0 0 0 14 0M12 18v3"></path>'
CLIP = '<path d="M20 11l-8 8a5 5 0 0 1-7-7l8-8a3.5 3.5 0 0 1 5 5l-8 8a2 2 0 0 1-3-3l7-7"></path>'
SMILE = '<circle cx="12" cy="12" r="9"></circle><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01"></path>'
SEARCH = '<circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path>'
CHEVL = '<path d="M15 6l-6 6 6 6"></path>'
UP = '<path d="M7 14l5-5 5 5"></path>'
PLUS = '<path d="M12 6v12M6 12h12"></path>'
STOP = '<rect x="7" y="7" width="10" height="10" rx="2"></rect>'
MORE = ('<svg width="20" height="20" viewBox="0 0 24 24" fill="#E6E7EA" aria-hidden="true"><circle cx="12" cy="5" r="1.8"></circle>'
        '<circle cx="12" cy="12" r="1.8"></circle><circle cx="12" cy="19" r="1.8"></circle></svg>')
TICKS = ('<svg width="16" height="10" viewBox="0 0 18 12" fill="none" stroke="#9AD7EE" stroke-width="1.8" stroke-linecap="round" '
         'stroke-linejoin="round" aria-label="Доставлено" role="img"><path d="M1.5 6.5l3.2 3.2L11 2.5"></path><path d="M8.2 9.2l.5.5L15 2.5"></path></svg>')
TICKS_G = TICKS.replace('#9AD7EE', '#8E8E98')


def btn(path, label, color="#E6E7EA", bg=""):
    st = f' style="background:{bg};"' if bg else ' style="background:none;"'
    return f'<button class="ib"{st} aria-label="{label}">{ic(path, 20, color)}</button>'


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


def chat_header(name="Алиса", status="в сети", color="#3949AB", sq=False):
    r = "14px" if sq else "50%"
    st_color = "#8BC34A" if status == "в сети" else "#8E8E98"
    return ('<div style="display: flex; align-items: center; gap: 10px; padding: 14px 12px 14px 8px;">'
            + btn('<path d="M15 5l-7 7 7 7"></path>', "Назад")
            + f'<div style="width: 40px; height: 40px; border-radius: {r}; background: {color}; color: #FFFFFF; font-size: 16px; font-weight: 500; display: flex; align-items: center; justify-content: center; flex: none;">{name[0]}</div>'
            + f'<div style="flex: 1; min-width: 0; display: flex; flex-direction: column;"><div style="font-size: 17px; font-weight: 500; color: #ECEDEF;">{name}</div>'
            + f'<div style="font-size: 12px; color: {st_color};">{status}</div></div>'
            + '<button class="ib" aria-label="Аудиозвонок">' + ic('<path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2"></path>') + '</button>'
            + f'<button class="ib" aria-label="Ещё">{MORE}</button></div>')


def sel_header(count, actions):
    return ('<div style="display: flex; align-items: center; gap: 4px; padding: 14px 8px 14px 8px;">'
            + btn(CLOSE, "Снять выделение")
            + f'<div style="flex: 1; font-size: 18px; font-weight: 500; color: #ECEDEF; padding-left: 6px;">{count}</div>'
            + ''.join(actions) + '</div>')


PANEL = '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; overflow: hidden;">'
FEED_OPEN = '<div style="flex: 1; display: flex; flex-direction: column; justify-content: flex-end; gap: 3px; padding: 12px 14px;">'
SP = '<div style="height: 6px;"></div>'


def composer(field=None, right=None):
    field = field or (ic(CLIP, 20, "#9AA3AD", NF) + '<span class="ph">Сообщение</span>' + ic(SMILE, 20, "#9AA3AD", NF))
    right = right or f'<button class="snd" aria-label="Голосовое">{ic(MIC, 20, "#04222E")}</button>'
    return f'<div class="cmp"><div class="fld">{field}</div>{right}</div>'


# Сообщения ленты по умолчанию
M = {
    "in1": '<div class="bin s1">Ты сегодня дома?<span class="mt">14:18</span></div>',
    "in2": '<div class="bin s2">Хочу занести книгу<span class="mt">14:18</span></div>',
    "out1": f'<div class="bout o1"><div class="q"><div style="color:#9AD7EE;font-weight:500;">Алиса</div>Хочу занести книгу</div>да, после семи<span class="mto">14:19 {TICKS}</span></div>',
    "out2": f'<div class="bout o2">ок, жду<span class="mto">14:22 {TICKS}</span></div>',
    "in3": '<div class="bin s1">Отлично. Какой код у домофона?<span class="mt">14:25</span></div>',
}


def feed(*keys, wrap=None):
    wrap = wrap or {}
    out = [FEED_OPEN, '<div class="lbl">Сегодня</div>']
    prev = None
    for k in keys:
        if prev and prev[:2] != k[:2]:
            out.append(SP)
        html = M[k]
        if k in wrap:
            html = wrap[k](html)
        out.append(html)
        prev = k
    out.append('</div>')
    return ''.join(out)


def selrow(html):
    return f'<div class="sel">{html}</div>'


# ---------- 1. А · Панель вместо шапки ----------
a_actions = [btn(REPLY, "Ответить"), btn(FWD, "Переслать"), btn(COPY, "Копировать"), btn(TRASH, "Удалить"),
             f'<button class="ib" style="background:none;" aria-label="Ещё">{MORE}</button>']
act_a = page("Действия: панель вместо шапки", sel_header("1", a_actions) + PANEL
             + feed("in1", "in2", "out1", "out2", "in3", wrap={"out2": selrow}) + composer() + '</div>'
             + '<div style="position:absolute;right:14px;top:62px;width:236px;" class="menu">'
             + ''.join(f'<button class="mi">{ic(p, 20, "#9AA3AD", NF)}{t}</button>' for p, t in
                       [(EDIT, "Изменить"), (PIN, "Закрепить"), (SAVE, "В «Сохранённое»")])
             + '</div>')


# ---------- 2. Б · Меню у пузыря ----------
def menu_items(own=True):
    items = [(REPLY, "Ответить"), (COPY, "Копировать"), (FWD, "Переслать"), (PIN, "Закрепить")]
    if own:
        items.append((EDIT, "Изменить"))
    items += [(SAVE, "В «Сохранённое»"), (SELECT, "Выбрать")]
    h = ''.join(f'<button class="mi">{ic(p, 20, "#9AA3AD", NF)}{t}</button>' for p, t in items)
    return h + '<div class="mdv"></div>' + f'<button class="mi red">{ic(TRASH, 20, "#FF6B6B", NF)}Удалить</button>'


act_b = page("Действия: меню у пузыря", chat_header() + PANEL
             + feed("in1", "in2", "out1", "out2", "in3") + composer() + '</div>'
             + '<div class="dim"></div>'
             + '<div style="position:absolute;right:14px;top:146px;display:flex;align-items:center;gap:2px;padding:4px;background:#1C1D22;border-radius:24px;box-shadow:0 12px 32px rgba(0,0,0,0.5),0 0 0 1px rgba(255,255,255,0.04);">'
             + ''.join(f'<button class="rx" aria-label="Реакция {e}">{e}</button>' for e in ["👍", "❤️", "😂", "😮", "😢", "🙏"])
             + f'<button class="rx" aria-label="Другая реакция" style="background:#26272D;">{ic(PLUS, 18, "#E6E7EA")}</button></div>'
             + f'<div style="position:absolute;right:14px;top:206px;" class="bout o2">ок, жду<span class="mto">14:22 {TICKS}</span></div>'
             + '<div style="position:absolute;right:14px;top:252px;width:250px;" class="menu">'
             + f'<div style="display:flex;align-items:center;gap:8px;padding:12px 16px 10px;font-size:13px;color:#8E8E98;">{TICKS_G}Доставлено · сегодня, 14:22</div>'
             + '<div class="mdv"></div>' + menu_items() + '</div>')

# ---------- 3. В · Лист снизу ----------
tiles = ''.join(f'<button class="tile">{ic(p, 22)}{t}</button>' for p, t in
                [(REPLY, "Ответить"), (COPY, "Копировать"), (FWD, "Переслать"), (PIN, "Закрепить")])
rows_v = ''.join(f'<button class="mi" style="padding:0 4px;">{ic(p, 20, "#9AA3AD", NF)}{t}</button>' for p, t in
                 [(EDIT, "Изменить"), (SAVE, "В «Сохранённое»"), (SELECT, "Выбрать")])
act_c = page("Действия: лист снизу", chat_header() + PANEL
             + feed("in1", "in2", "out1", "out2", "in3", wrap={"out2": selrow}) + composer() + '</div>'
             + '<div class="dim" style="background:rgba(0,0,0,0.45);"></div>'
             + '<div class="sheet"><div class="hdl"></div>'
             + f'<div style="display:flex;align-items:center;gap:8px;font-size:13px;color:#8E8E98;padding:4px 4px 0;">{TICKS_G}Доставлено · сегодня, 14:22</div>'
             + f'<div style="display:flex;gap:8px;">{tiles}</div>'
             + f'<div style="display:flex;flex-direction:column;">{rows_v}<div class="mdv"></div>'
             + f'<button class="mi red" style="padding:0 4px;">{ic(TRASH, 20, "#FF6B6B", NF)}Удалить</button></div>'
             + '</div>')


# ---------- 4. Выбор нескольких ----------
def ckrow(html, on):
    ck = f'<span class="ck on">{ic(CHECK, 14, "#04222E")}</span>' if on else '<span class="ck"></span>'
    cls = "sel" if on else "row"
    return f'<div class="{cls}" style="position:relative;padding-left:48px;">{ck}{html}</div>'


multi_feed = (FEED_OPEN + '<div class="lbl">Сегодня</div>'
              + ckrow(M["in1"], True) + ckrow(M["in2"], True) + SP
              + ckrow(M["out1"], False) + ckrow(M["out2"], True) + SP + ckrow(M["in3"], False) + '</div>')
multi = page("Действия: выбор нескольких", sel_header("Выбрано: 3", [btn(FWD, "Переслать"), btn(COPY, "Копировать"), btn(TRASH, "Удалить")])
             + PANEL + multi_feed
             + ''
             + '</div>')

# ---------- 5. Ответ свайпом ----------
swipe_feed = (FEED_OPEN + '<div class="lbl">Сегодня</div>' + M["in1"]
              + '<div style="position:relative;display:flex;align-items:center;">'
              + '<div class="bin s2" style="transform:translateX(-0px);margin-left:0;">Хочу занести книгу<span class="mt">14:18</span></div></div>'
              + SP + M["out1"] + M["out2"] + SP
              + '<div style="position:relative;display:flex;align-items:center;justify-content:flex-end;">'
              + f'<div style="position:absolute;right:6px;width:32px;height:32px;border-radius:50%;background:#26272D;display:flex;align-items:center;justify-content:center;">{ic(REPLY, 18, "#7FD0EE")}</div>'
              + '<div class="bin s1" style="margin-right:56px;">Отлично. Какой код у домофона?<span class="mt">14:25</span></div></div>'
              + '</div>')
swipe = page("Действия: ответ свайпом", chat_header() + PANEL + swipe_feed
             + '<div class="strip">' + ic(REPLY, 18, "#33B5E5", NF)
             + '<div class="sti"><div class="stt">Ответ Алисе</div><div class="sts">Отлично. Какой код у домофона?</div></div>'
             + btn(CLOSE, "Отменить ответ", "#9AA3AD") + '</div>'
             + composer(ic(CLIP, 20, "#9AA3AD", NF) + '<span class="ph">Сообщение<span class="cr"></span></span>' + ic(SMILE, 20, "#9AA3AD", NF))
             + '</div>')

# ---------- 6. Удаление ----------
delete = page("Действия: удаление", sel_header("Выбрано: 2", [btn(FWD, "Переслать"), btn(COPY, "Копировать"), btn(TRASH, "Удалить")])
              + PANEL + feed("in1", "in2", "out1", "out2", "in3", wrap={"out1": selrow, "out2": selrow}) + '<div style="height:78px;"></div></div>'
              + '<div class="dim" style="background:rgba(0,0,0,0.45);"></div>'
              + '<div class="sheet"><div class="hdl"></div><div class="sht">Удалить 2 сообщения?</div>'
              + '<div class="sub" style="padding:0 4px;">«У всех» — просьба к собеседнику удалить их у себя. Если сообщения уже прочитаны или сохранены, это не отменить.</div>'
              + '<div style="display:flex;flex-direction:column;gap:8px;padding-top:4px;">'
              + '<button class="btn red">Удалить у всех</button><button class="btn">Удалить у себя</button>'
              + '<button class="btn" style="background:none;color:#B8B8C0;">Отмена</button></div></div>')


# ---------- 7. Пересылка ----------
def lrow(av, color, name, on=False, sq=False, sub=""):
    rc = f'<span class="rc on">{ic(CHECK, 14, "#04222E")}</span>' if on else '<span class="rc"></span>'
    r = "12px" if sq else "50%"
    s = f'<span class="sub" style="display:block;font-weight:400;">{sub}</span>' if sub else ''
    return (f'<div class="lr"><div class="av" style="background:{color};border-radius:{r};">{av}</div>'
            f'<b>{name}{s}</b>{rc}</div>')


fwd = page("Действия: пересылка", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3") + '</div>'
           + '<div class="dim" style="background:rgba(0,0,0,0.45);"></div>'
           + '<div class="sheet" style="top:120px;gap:10px;"><div class="hdl"></div><div class="sht">Переслать</div>'
           + f'<div class="fld" style="min-height:44px;background:#26272D;border-radius:14px;">{ic(SEARCH, 18, "#9AA3AD", NF)}<span class="ph">Кому</span></div>'
           + '<div style="display:flex;flex-direction:column;">'
           + lrow("С", "#2B5F73", "Сохранённое", sq=True)
           + lrow("Б", "#00897B", "Борис", on=True)
           + lrow("Д", "#5D4037", "Дача", on=True, sq=True, sub="Группа · 4 участника")
           + lrow("М", "#8E24AA", "Мария")
           + lrow("К", "#C2185B", "Кирилл")
           + '</div><div style="flex:1;"></div>'
           + '<div style="display:flex;align-items:center;gap:14px;padding:4px 4px;"><div style="flex:1;"><div style="font-size:15px;color:#ECEDEF;">Скрыть автора</div>'
           + '<div class="sub">Получатель увидит «Переслано», но не узнает, от кого</div></div><span class="tg"><i></i></span></div>'
           + '<button class="btn pri">Отправить в 2 чата</button></div>')

# ---------- 8. Ответ ----------
reply = page("Ввод: ответ", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3")
             + '<div class="strip">' + ic(REPLY, 18, "#33B5E5", NF)
             + '<div class="sti"><div class="stt">Ответ Алисе</div><div class="sts">Отлично. Какой код у домофона?</div></div>'
             + btn(CLOSE, "Отменить ответ", "#9AA3AD") + '</div>'
             + composer(ic(CLIP, 20, "#9AA3AD", NF) + '<span class="tx">2580, потом<span class="cr"></span></span>' + ic(SMILE, 20, "#9AA3AD", NF),
                        f'<button class="snd" aria-label="Отправить">{ic(SEND, 20, "#04222E")}</button>')
             + '</div>')

# ---------- 9. Изменение ----------
edit = page("Ввод: изменение", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3", wrap={"out2": selrow})
            + '<div class="strip">' + ic(EDIT, 18, "#33B5E5", NF)
            + '<div class="sti"><div class="stt">Изменение</div><div class="sts">ок, жду</div></div>'
            + btn(CLOSE, "Отменить изменение", "#9AA3AD") + '</div>'
            + composer(ic(SMILE, 20, "#9AA3AD", NF) + '<span class="tx">ок, жду внизу<span class="cr"></span></span>',
                       f'<button class="snd" aria-label="Сохранить">{ic(CHECK, 20, "#04222E")}</button>')
            + '</div>')

# ---------- 10. Голосовое: удержание ----------
wave = lambda n, c: ''.join(f'<i style="width:3px;height:{h}px;background:{c};border-radius:2px;flex:none;"></i>' for h in ([6, 12, 18, 9, 16, 22, 14, 8, 17, 11, 20, 7, 13, 19, 10, 15, 6, 12, 18, 9][:n]))
hold = page("Ввод: голосовое, удержание", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3")
            + '<div class="cmp" style="align-items:center;position:relative;">'
            + '<div class="fld" style="gap:10px;"><span style="width:10px;height:10px;border-radius:50%;background:#FF6B6B;flex:none;"></span>'
            + '<span style="font-size:15px;color:#ECEDEF;font-variant-numeric:tabular-nums;">0:07</span>'
            + f'<span style="flex:1;display:flex;align-items:center;justify-content:center;gap:4px;font-size:14px;color:#8E8E98;">{ic(CHEVL, 16, "#8E8E98")}Влево — отмена</span></div>'
            + '<div style="width:46px;height:46px;flex:none;"></div>'
            + '<div style="position:absolute;right:-2px;bottom:4px;width:76px;height:76px;border-radius:24px;background:#33B5E5;display:flex;align-items:center;justify-content:center;">'
            + ic(MIC, 26, "#04222E") + '</div>'
            + '<div style="position:absolute;right:15px;bottom:92px;width:42px;padding:10px 0;border-radius:21px;background:#1C1D22;display:flex;flex-direction:column;align-items:center;gap:6px;box-shadow:0 0 0 1px rgba(255,255,255,0.06);">'
            + ic(LOCK, 18, "#E6E7EA") + ic(UP, 16, "#8E8E98") + '</div>'
            + '</div></div>')

# ---------- 11. Голосовое: зафиксировано ----------
locked = page("Ввод: голосовое, зафиксировано", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3")
              + '<div class="cmp" style="align-items:center;">'
              + btn(TRASH, "Удалить запись", "#FF8A8A", "#1C1D22")
              + '<div class="fld" style="gap:10px;"><span style="width:10px;height:10px;border-radius:50%;background:#FF6B6B;flex:none;"></span>'
              + '<span style="font-size:15px;color:#ECEDEF;font-variant-numeric:tabular-nums;">0:23</span>'
              + f'<span style="flex:1;display:flex;align-items:center;gap:2px;overflow:hidden;height:24px;">{wave(20, "#9AA3AD")}</span>'
              + f'<button class="ib" style="width:32px;height:32px;background:none;" aria-label="Остановить запись">{ic(STOP, 18, "#E6E7EA")}</button></div>'
              + f'<button class="snd" aria-label="Отправить">{ic(SEND, 20, "#04222E")}</button>'
              + '</div></div>')

# ---------- 12. Прикрепить ----------
grads = ["#2F4A3A,#4B6B52", "#3A2F4A,#5B4B6B", "#4A3A2F,#6B5B4B", "#2F3A4A,#4B5B6B", "#4A2F35,#6B4B55",
         "#2F4A47,#4B6B67", "#3F3F2F,#62624B", "#2F2F4A,#4B4B6B"]
cells = [f'<button class="ph3" style="background:#26272D;border:none;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;color:#ECEDEF;font-family:inherit;font-size:13px;box-shadow:none;">{ic(CAMERA, 24)}Камера</button>']
RING = '<span class="ring"></span>'
marks = {1: '<span class="num">1</span>', 3: '<span class="num">2</span>'}
for i, g in enumerate(grads):
    cells.append(f'<div class="ph3" style="background:linear-gradient(135deg,{g});">{marks.get(i, RING)}</div>')
attach = page("Ввод: прикрепить", chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3") + '</div>'
              + '<div class="dim" style="background:rgba(0,0,0,0.45);"></div>'
              + '<div class="sheet" style="gap:12px;"><div class="hdl"></div>'
              + f'<div style="display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;">{"".join(cells)}</div>'
              + '<div style="display:flex;gap:8px;">'
              + f'<button class="btn" style="flex:1;">{ic(FILE, 20)}Файл</button><button class="btn" style="flex:1;">{ic(MUSIC, 20)}Аудио</button></div>'
              + '<button class="btn pri">Отправить 2 фото · 3,4 МБ</button></div>')

# ---------- 13. Упоминание ----------
gfeed = (FEED_OPEN + '<div class="lbl">Сегодня</div>'
         + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#4DB6AC;margin-bottom:2px;">Борис</div>Кто в субботу едет?<span class="mt">19:02</span></div>'
         + '<div class="bin s1" style="margin-top:6px;"><div style="font-size:13px;font-weight:500;color:#CE93D8;margin-bottom:2px;">Мария</div>Я, но после обеда<span class="mt">19:04</span></div>'
         + '</div>')
mention = page("Ввод: упоминание", chat_header("Дача", "4 участника", "#5D4037", sq=True) + PANEL + gfeed
               + '<div style="margin:0 12px 6px;" class="menu">'
               + '<div class="lr" style="padding:0 14px;"><div class="av" style="background:#00897B;width:32px;height:32px;font-size:14px;">Б</div><b>Борис</b></div>'
               + '<div class="mdv" style="margin-left:58px;"></div>'
               + '<div class="lr" style="padding:0 14px;"><div class="av" style="background:#6D4C41;width:32px;height:32px;font-size:14px;">Б</div><b>Богдан</b></div>'
               + '</div>'
               + composer(ic(CLIP, 20, "#9AA3AD", NF) + '<span class="tx">Тогда @бо<span class="cr"></span></span>' + ic(SMILE, 20, "#9AA3AD", NF),
                          f'<button class="snd" aria-label="Отправить">{ic(SEND, 20, "#04222E")}</button>')
               + '</div>')

# ---------- Реакции в ленте (группа) ----------
def chips(*items):
    return '<div class="chips">' + ''.join(
        f'<button class="rc2{" me" if me else ""}"><span>{e}</span>{n}</button>' for e, n, me in items) + '</div>'


rfeed = (FEED_OPEN + '<div class="lbl">Сегодня</div>'
         + '<div style="align-self:flex-start;display:flex;flex-direction:column;align-items:flex-start;">'
         + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#4DB6AC;margin-bottom:2px;">Борис</div>Шашлык беру на себя<span class="mt">19:02</span></div>'
         + chips(("👍", 3, True), ("😂", 1, False)) + '</div>'
         + '<div style="align-self:flex-start;display:flex;flex-direction:column;align-items:flex-start;margin-top:6px;">'
         + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#CE93D8;margin-bottom:2px;">Мария</div>Я привезу мангал, у вашего дно прогорело<span class="mt">19:04</span></div>'
         + chips(("😮", 1, False)) + '</div>'
         + SP
         + '<div style="align-self:flex-end;display:flex;flex-direction:column;align-items:flex-end;">'
         + f'<div class="bout o1">Тогда я — уголь и розжиг<span class="mto">19:06 {TICKS}</span></div>'
         + chips(("❤️", 2, False), ("🙏", 1, False)) + '</div>'
         + '</div>')
react = page("Реакции в ленте", chat_header("Дача", "4 участника", "#5D4037", sq=True) + PANEL + rfeed + composer() + '</div>')


# ---------- Кто отреагировал ----------
def who(av, color, name, e):
    return (f'<div class="lr"><div class="av" style="background:{color};">{av}</div><b>{name}</b>'
            f'<span style="font-size:20px;">{e}</span></div>')


react_who = page("Кто отреагировал", chat_header("Дача", "4 участника", "#5D4037", sq=True) + PANEL + rfeed + '</div>'
                 + '<div class="dim" style="background:rgba(0,0,0,0.45);"></div>'
                 + '<div class="sheet"><div class="hdl"></div><div class="sht">Реакции</div>'
                 + '<div class="tabs"><button class="tb on">Все 4</button><button class="tb"><span style="font-size:16px;">👍</span>3</button>'
                 + '<button class="tb"><span style="font-size:16px;">😂</span>1</button></div>'
                 + '<div style="display:flex;flex-direction:column;">'
                 + who("Я", "#2B5F73", "Вы", "👍") + who("М", "#8E24AA", "Мария", "👍")
                 + who("К", "#C2185B", "Кирилл", "👍") + who("Б", "#6D4C41", "Богдан", "😂")
                 + '</div></div>')

rows = [
    [("Main.dc.html", act_a, "А · Панель вместо шапки (как сейчас): иконки + «⋮»"),
     ("ActMenu.dc.html", act_b, "Б · Меню у пузыря ✓: реакции, подписи, статус доставки"),
     ("ActSheet.dc.html", act_c, "В · Лист снизу: 4 плитки + список")],
    [("Multi.dc.html", multi, "Выбор нескольких (из «Выбрать»): ряд целиком + кружки"),
     ("Swipe.dc.html", swipe, "Ответ свайпом влево по пузырю"),
     ("Delete.dc.html", delete, "Удаление: лист, «у всех» — просьба"),
     ("Forward.dc.html", fwd, "Пересылка: несколько чатов, «Скрыть автора»"),
     ("Reactions.dc.html", react, "Реакции под пузырём: своя — обведена"),
     ("ReactWho.dc.html", react_who, "Кто отреагировал: долгое нажатие на реакцию")],
    [("Reply.dc.html", reply, "Ввод · ответ"),
     ("Edit.dc.html", edit, "Ввод · изменение"),
     ("VoiceHold.dc.html", hold, "Ввод · голосовое: удержание (вверх — замок)"),
     ("VoiceLocked.dc.html", locked, "Ввод · голосовое: зафиксировано"),
     ("Attach.dc.html", attach, "Ввод · прикрепить: камера, снимки, файл, аудио"),
     ("Mention.dc.html", mention, "Ввод · упоминание: участники по «@»")],
]
row_titles = ["Действия с сообщением: как открываются", "Что дальше: несколько, свайп, удаление, пересылка, реакции", "Поле ввода"]
boards, order, notes = {}, [], {}
for ri, r in enumerate(rows):
    y = ri * 1235
    for ci, (name, html, title) in enumerate(r):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(html)
        boards[name] = {"x": ci * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
    notes[f"t{ri + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[ri], "kind": "title1", "maxW": len(r) * 492 - 80}
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T20:00:00Z"}, "title": "Действия в чате Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes, "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok", len(order))
