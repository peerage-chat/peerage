import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
# Общие куски (CSS ленты, иконки, page, шапка, поле ввода) — из генератора «Особых».
src = open(os.path.join(HERE, "gen_special.py"), encoding="utf-8").read()
exec(src[:src.index("# ---------- 1. Статусы отправки")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

R, S = 18, 6  # полный угол и срез на стыке
NEAR = 18     # разница ширин меньше полного угла — края считаются вровень


def corners(widths, mine, adaptive):
    """Углы пузырей одной серии: (верх-лево, верх-право, низ-право, низ-лево) каждого.

    Ведущая сторона (у входящих — левая, у своих — правая) на стыках всегда срезана.
    Дальняя — по соседу: сосед выступает дальше, или края почти вровень, — угол на этом
    стыке срезан; выступает сам пузырь — угол полный. Без `adaptive` — как сейчас:
    дальняя сторона всегда полная.
    """
    n = len(widths)
    out = []
    for i, w in enumerate(widths):
        top_lead = S if i > 0 else R
        bot_lead = S if i < n - 1 else R
        top_far = bot_far = R
        if adaptive:
            if i > 0 and widths[i - 1] - w > -NEAR:
                top_far = S
            if i < n - 1 and widths[i + 1] - w > -NEAR:
                bot_far = S
        if mine:
            out.append((top_far, top_lead, bot_lead, bot_far))
        else:
            out.append((top_lead, top_far, bot_far, bot_lead))
    return out


def series(items, mine, adaptive):
    cs = corners([w for w, _, _ in items], mine, adaptive)
    cls, tcls = ("bout", "mto") if mine else ("bin", "mt")
    mark = f" {TICKS}" if mine else ""
    html = ""
    for (w, text, time), (a, b, c, d) in zip(items, cs):
        html += (f'<div class="{cls}" style="width:{w}px;box-sizing:border-box;border-radius:{a}px {b}px {c}px {d}px;">'
                 f'{text}<span class="{tcls}" style="float:right;margin-top:4px;">{time}{mark}</span></div>')
    return html


IN = [(290, "Завтра в десять у входа в парк, возле фонтана. Если опоздаю — напишу", "19:01"),
      (128, "Успеешь?", "19:02"),
      (208, "Возьми воду и плед", "19:02"),
      (80, "Ок", "19:03")]
MY = [(146, "Да, буду", "19:05"),
      (154, "До завтра", "19:05"),
      (290, "Плед возьму, воду купим по дороге у метро", "19:06")]


def feed(adaptive):
    return (FEED_OPEN + LBL("Сегодня") + series(IN, False, adaptive) + SP + series(MY, True, adaptive) + '</div>')


now = page("Пузыри: сейчас", chat_header() + PANEL + feed(False) + composer() + '</div>')
new = page("Пузыри: дальняя сторона по соседу", chat_header() + PANEL + feed(True) + composer() + '</div>')

rows = [[("Main.dc.html", now, "Сейчас: дальняя сторона всегда скруглена"),
         ("Adaptive.dc.html", new, "Предлагаю: угол у стыка срезан, если сосед шире или вровень")]]
row_titles = ["Углы пузырей в серии"]
boards, order, notes = {}, [], {}
for ri, r in enumerate(rows):
    y = ri * 1235
    for ci, (fn, html, title) in enumerate(r):
        with open(os.path.join(P, fn), "w", encoding="utf-8") as f:
            f.write(html)
        boards[fn] = {"x": ci * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(fn)
    notes[f"t{ri + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[ri], "kind": "title1", "maxW": len(r) * 492 - 80}
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-22T13:00:00Z"}, "title": "Углы пузырей Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes, "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok", len(order))
