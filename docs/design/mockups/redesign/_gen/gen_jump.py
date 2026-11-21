import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
# Общие куски (CSS, иконки, page, шапка, поле ввода, .fab/.bdg/.unr) — из генератора «Особых».
src = open(os.path.join(HERE, "gen_special.py"), encoding="utf-8").read()
exec(src[:src.index("# ---------- 1. Статусы отправки")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.cap2{position:absolute;left:16px;right:78px;font-size:12px;line-height:16px;color:#8E8E98}
.pill2{position:absolute;right:16px;width:46px;border-radius:23px;background:#26272D;display:flex;flex-direction:column;align-items:center;box-shadow:0 6px 18px rgba(0,0,0,0.45),0 0 0 1px rgba(255,255,255,0.05);overflow:hidden}
.pill2 button{width:46px;min-height:52px;border:none;background:none;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;padding:6px 0;font-family:inherit}
.pill2 button + button{box-shadow:inset 0 1px 0 #34353C}
.pill2 b{font-size:11px;font-weight:700;color:#7FD0EE;font-variant-numeric:tabular-nums}
.bar{display:flex;align-items:center;gap:8px;margin:0 12px 0;padding:0 4px}
.chp{display:flex;align-items:center;gap:6px;min-height:36px;padding:0 12px;border-radius:18px;border:none;background:#26272D;color:#ECEDEF;font-family:inherit;font-size:14px;box-shadow:0 4px 12px rgba(0,0,0,0.35)}
.chp b{font-weight:700;color:#7FD0EE;font-variant-numeric:tabular-nums}
.rc2.new{box-shadow:inset 0 0 0 1.5px #7FD0EE;background:#1F3A46;color:#EAF6FB}
"""

HEART = '<path d="M19.5 12.6L12 20l-7.5-7.4A5 5 0 1 1 12 6a5 5 0 1 1 7.5 6.6z"></path>'


def name(n, c):
    return f'<div style="font-size:13px;font-weight:500;color:{c};margin-bottom:2px;">{n}</div>'


def chips(*items):
    return '<div class="chips">' + ''.join(
        f'<button class="rc2{" " + cls if cls else ""}"><span>{e}</span>{n}</button>' for e, n, cls in items) + '</div>'


def mine(text, time, ch=None, row=False):
    inner = f'<div class="bout">{text}<span class="mto">{time} {TICKS}</span></div>' + (ch or "")
    col = wrapcol(inner)
    return f'<div class="sel" style="align-items:flex-end;">{col}</div>' if row else col


def theirs(n, c, text, time, ch=None, row=False, quote=None, s="s1"):
    q = f'<div class="q"><div style="color:#7FD0EE;font-weight:500;">{quote[0]}</div>{quote[1]}</div>' if quote else ""
    inner = f'<div class="bin {s}">{name(n, c) if n else ""}{q}{text}<span class="mt">{time}</span></div>' + (ch or "")
    col = wrapcol(inner, "start")
    return f'<div class="sel" style="align-items:flex-start;">{col}</div>' if row else col


def fab(bottom, path, label, n=None, size=20):
    b = f'<span class="bdg">{n}</span>' if n else ""
    return f'<button class="fab" style="bottom:{bottom}px;" aria-label="{label}">{ic(path, size)}{b}</button>'


GROUP = chat_header("Дача 2026", "4 участника", "#5D4037", sq=True)
B, M, K = ("Борис", "#4DB6AC"), ("Мария", "#CE93D8"), ("Кирилл", "#F48FB1")

# Лента «далеко вверху»: свои сообщения с реакциями, ниже — чужие.
feed_up = (FEED_OPEN + LBL("Вчера")
           + mine("Ключ оставлю у соседки, кв. 14", "18:20", chips(("👍", 2, ""), ("🙏", 1, "")))
           + SP + theirs(*B, "Ок, заберу после работы", "18:31")
           + theirs(None, "", "Там же и зарядка?", "18:32", s="s2")
           + SP + mine("Да, в пакете", "18:40", chips(("❤️", 1, "")))
           + SP + theirs(*M, "Кто везёт мангал?", "19:05")
           + '</div>')

# ---------- Варианты стопки ----------
STACK_A = (fab(196, HEART, "К новым реакциям: 2", 2)
           + fab(140, AT, "К упоминанию: 1", 1)
           + fab(84, CHEVD, "Вниз, непрочитанных: 3", 3, 22))
var_a = page("Переходы: столбик кнопок", GROUP + PANEL + feed_up + composer() + '</div>' + STACK_A)

STACK_B = ('<div class="pill2" style="bottom:84px;">'
           + f'<button aria-label="К новым реакциям: 2">{ic(HEART, 20)}<b>2</b></button>'
           + f'<button aria-label="К упоминанию: 1">{ic(AT, 20)}<b>1</b></button>'
           + f'<button aria-label="Вниз, непрочитанных: 3">{ic(CHEVD, 22)}<b>3</b></button>'
           + '</div>')
var_b = page("Переходы: одна капсула", GROUP + PANEL + feed_up + composer() + '</div>' + STACK_B)

BAR_C = ('<div class="bar" style="position:absolute;left:12px;bottom:90px;">'
         + f'<button class="chp" aria-label="К новым реакциям: 2">{ic(HEART, 18)}<b>2</b></button>'
         + f'<button class="chp" aria-label="К упоминанию: 1">{ic(AT, 18)}<b>1</b></button></div>'
         + fab(84, CHEVD, "Вниз, непрочитанных: 3", 3, 22))
var_c = page("Переходы: чипы слева, вниз справа", GROUP + PANEL + feed_up + composer() + '</div>' + BAR_C)

# ---------- Переход к реакции ----------
feed_react = (FEED_OPEN + LBL("Вчера")
              + mine("Ключ оставлю у соседки, кв. 14", "18:20", chips(("👍", 2, ""), ("🙏", 1, "new")), row=True)
              + SP + theirs(*B, "Ок, заберу после работы", "18:31")
              + theirs(None, "", "Там же и зарядка?", "18:32", s="s2")
              + SP + mine("Да, в пакете", "18:40", chips(("❤️", 1, "")))
              + SP + theirs(*M, "Кто везёт мангал?", "19:05")
              + '</div>')
react = page("Переходы: к новой реакции", GROUP + PANEL + feed_react + composer() + '</div>'
             + fab(196, HEART, "К новым реакциям: 1", 1)
             + fab(140, AT, "К упоминанию: 1", 1)
             + fab(84, CHEVD, "Вниз, непрочитанных: 3", 3, 22))

# ---------- Цитаты: туда и обратно ----------
feed_quote_src = (FEED_OPEN + LBL("12 ноября")
                  + theirs(*K, "Шланг для полива — 25 метров хватит?", "11:02", row=True)
                  + SP + mine("Хватит, у бочки кран", "11:10")
                  + SP + theirs(*B, "Беру 30, с запасом", "11:14")
                  + SP + LBL("Сегодня")
                  + theirs(*M, "Так какой шланг берём?", "20:01", quote=("Кирилл", "Шланг для полива — 25 метров…"))
                  + '</div>')
quote1 = page("Переходы: по цитате к исходному", GROUP + PANEL + feed_quote_src + composer() + '</div>'
              + fab(84, CHEVD, "Назад к ответу", None, 22))

feed_quote_back = (FEED_OPEN + LBL("Сегодня")
                   + theirs(*M, "Так какой шланг берём?", "20:01", quote=("Кирилл", "Шланг для полива — 25 метров…"), row=True)
                   + SP + mine("30, Борис уже купил", "20:03")
                   + SP + theirs(*K, "Отлично", "20:05")
                   + theirs(None, "", "Тогда за мной лейки", "20:05", s="s2")
                   + SP + theirs(*B, "И перчатки", "20:07")
                   + '</div>')
quote2 = page("Переходы: «вниз» — к ответу", GROUP + PANEL + feed_quote_back + composer() + '</div>'
              + fab(84, CHEVD, "Вниз", None, 22))

rows = [
    [("Main.dc.html", var_a, "А · Столбик: реакции · @ · вниз (как Telegram)"),
     ("Capsule.dc.html", var_b, "Б · Одна капсула, числа внутри"),
     ("Chips.dc.html", var_c, "В · Реакции и @ — чипами слева, «вниз» справа")],
    [("React.dc.html", react, "К новой реакции: подсветка ряда, новая реакция обведена; счётчик −1"),
     ("QuoteTo.dc.html", quote1, "По цитате к исходному: место запомнено"),
     ("QuoteBack.dc.html", quote2, "«Вниз» — к ответу, с которого ушли; пустой стек — в хвост")],
]
row_titles = ["Стопка кнопок справа внизу — варианты", "Переходы: реакции, цитаты, возврат"]
boards, order, notes = {}, [], {}
for ri, r in enumerate(rows):
    y = ri * 1235
    for ci, (fn, html, title) in enumerate(r):
        with open(os.path.join(P, fn), "w", encoding="utf-8") as f:
            f.write(html)
        boards[fn] = {"x": ci * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(fn)
    notes[f"t{ri + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[ri], "kind": "title1", "maxW": len(r) * 492 - 80}
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-22T12:00:00Z"}, "title": "Переходы по ленте Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes, "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok", len(order))
