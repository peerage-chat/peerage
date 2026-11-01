import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
src = open(os.path.join(HERE, "gen_chatact.py"), encoding="utf-8").read()
# Общие куски (CSS, иконки, page, шапка чата, лента) — из генератора «Действий в чате».
exec(src[:src.index("# ---------- 1. А · Панель вместо шапки ----------")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.photo{position:absolute;left:0;right:0;top:250px;height:310px;background:linear-gradient(180deg,#56697B 0%,#8FA2AE 44%,#465C4C 45%,#33463A 70%,#253328 100%)}
.top{position:absolute;left:0;right:0;top:0;padding:14px 8px 40px;display:flex;align-items:center;gap:6px;background:linear-gradient(180deg,rgba(0,0,0,0.7),rgba(0,0,0,0))}
.bot{position:absolute;left:0;right:0;bottom:0;padding:48px 16px 20px;display:flex;flex-direction:column;gap:12px;background:linear-gradient(0deg,rgba(0,0,0,0.75),rgba(0,0,0,0))}
.who{flex:1;min-width:0;display:flex;flex-direction:column}
.who b{font-size:16px;font-weight:500;color:#FFFFFF}
.who span{font-size:12px;color:rgba(255,255,255,0.75);font-variant-numeric:tabular-nums}
.cap{font-size:15px;line-height:21px;color:#FFFFFF}
.acts{display:flex;justify-content:space-around}
.act{display:flex;flex-direction:column;align-items:center;gap:4px;min-width:64px;min-height:56px;justify-content:center;background:none;border:none;color:#FFFFFF;font-family:inherit;font-size:12px}
.tl{display:flex;align-items:center;gap:10px}
.trk{flex:1;height:4px;border-radius:2px;background:rgba(255,255,255,0.3);position:relative}
.trk i{position:absolute;left:0;top:0;bottom:0;width:26%;border-radius:2px;background:#FFFFFF}
.trk b{position:absolute;left:26%;top:50%;width:14px;height:14px;margin:-7px 0 0 -7px;border-radius:50%;background:#FFFFFF}
.tt{font-size:12px;color:#FFFFFF;font-variant-numeric:tabular-nums}
.wib{width:44px;height:44px;border-radius:14px;background:none;border:none;display:flex;align-items:center;justify-content:center;padding:0;flex:none}
"""

DL = '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"></path>'
SHARE = '<circle cx="6" cy="12" r="2.5"></circle><circle cx="18" cy="6" r="2.5"></circle><circle cx="18" cy="18" r="2.5"></circle><path d="M8.2 10.8l7.6-3.6M8.2 13.2l7.6 3.6"></path>'
INCHAT = '<path d="M5 5h14a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9l-4 3V6a1 1 0 0 1 1-1z"></path><path d="M9 10h6M9 13h4"></path>'
PLAY = '<path d="M8 5l11 7-11 7z"></path>'
PAUSE = '<path d="M8 5v14M16 5v14"></path>'
SOUND = '<path d="M4 10v4h4l5 4V6L8 10z"></path><path d="M16.5 9a4 4 0 0 1 0 6M19 6.5a7.5 7.5 0 0 1 0 11"></path>'
WMORE = ('<svg width="20" height="20" viewBox="0 0 24 24" fill="#FFFFFF" aria-hidden="true"><circle cx="12" cy="5" r="1.8"></circle>'
         '<circle cx="12" cy="12" r="1.8"></circle><circle cx="12" cy="19" r="1.8"></circle></svg>')


def wbtn(path, label, size=22):
    return f'<button class="wib" aria-label="{label}">{ic(path, size, "#FFFFFF")}</button>'


def top(more=True):
    return ('<div class="top">' + wbtn(CLOSE, "Закрыть")
            + '<div class="who"><b>Алиса</b><span>сегодня в 12:31 · 3 из 12</span></div>'
            + (f'<button class="wib" aria-label="Ещё">{WMORE}</button>' if more else '') + '</div>')


CAPTION = '<div class="cap">Туман с утра, к обеду обещали солнце</div>'
BLACK = '<div style="position:absolute;inset:0;background:#000000;"></div>'
PHOTO = '<div class="photo" role="img" aria-label="Фото"></div>'

# ---------- А · всё в «⋮» ----------
va = page("Просмотр: А, действия в меню", BLACK + PHOTO + top() + f'<div class="bot">{CAPTION}</div>')

# ---------- А · меню открыто ----------
items = [(REPLY, "Ответить"), (FWD, "Переслать"), (DL, "Сохранить"), (SHARE, "Поделиться"), (INCHAT, "Показать в чате")]
menu = ''.join(f'<button class="mi">{ic(p, 20, "#9AA3AD", NF)}{t}</button>' for p, t in items)
menu += '<div class="mdv"></div>' + f'<button class="mi red">{ic(TRASH, 20, "#FF6B6B", NF)}Удалить</button>'
vam = page("Просмотр: А, меню", BLACK + PHOTO + top() + f'<div class="bot">{CAPTION}</div>'
           + f'<div class="menu" style="position:absolute;right:12px;top:62px;width:236px;">{menu}</div>')

# ---------- Б · панель действий снизу ----------
acts = ''.join(f'<button class="act">{ic(p, 22, "#FFFFFF")}{t}</button>' for p, t in
               [(REPLY, "Ответить"), (FWD, "Переслать"), (DL, "Сохранить")])
acts += f'<button class="act">{WMORE}Ещё</button>'
vb = page("Просмотр: Б, панель снизу", BLACK + PHOTO + top(more=False)
          + f'<div class="bot">{CAPTION}<div class="acts">{acts}</div></div>')

# ---------- Видео ----------
vid = page("Просмотр: видео", BLACK + PHOTO.replace("Фото", "Видео") + top()
           + '<button style="position:absolute;left:50%;top:405px;width:64px;height:64px;margin:-32px 0 0 -32px;border-radius:50%;background:rgba(0,0,0,0.5);border:none;display:flex;align-items:center;justify-content:center;" aria-label="Пауза">'
           + ic(PAUSE, 26, "#FFFFFF") + '</button>'
           + '<div class="bot">' + CAPTION
           + f'<div class="tl"><span class="tt">0:12</span><div class="trk"><i></i><b></b></div><span class="tt">0:48</span>{wbtn(SOUND, "Выключить звук", 20)}</div></div>')

# ---------- Закрытие протяжкой вниз ----------
behind = chat_header() + PANEL + feed("in1", "in2", "out1", "out2", "in3") + composer() + '</div>'
close = page("Просмотр: закрытие протяжкой", behind
             + '<div style="position:absolute;inset:0;background:rgba(0,0,0,0.55);"></div>'
             + '<div class="photo" style="top:330px;left:36px;right:36px;height:262px;border-radius:18px;" role="img" aria-label="Фото уменьшается к пузырю"></div>')

rows = [("Main.dc.html", va, "А ✓ · Всё в «⋮», подпись снизу"),
        ("Menu.dc.html", vam, "А ✓ · Меню «⋮» (+ «Ответить»)"),
        ("Bar.dc.html", vb, "Б · Панель снизу — отвергнут"),
        ("Video.dc.html", vid, "Видео: пауза по центру, таймлайн внизу"),
        ("Close.dc.html", close, "Протяжка вниз: фон гаснет, снимок уходит в пузырь")]
boards, order = {}, []
for i, (name, html, title) in enumerate(rows):
    with open(os.path.join(P, name), "w", encoding="utf-8") as f:
        f.write(html)
    boards[name] = {"x": i * 492, "y": 0, "w": 412, "h": 892, "title": title}
    order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T22:00:00Z"}, "title": "Просмотр фото и видео Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order,
          "notes": {"t1": {"x": 0, "y": -300, "text": "Просмотр фото и видео", "kind": "title1", "maxW": 2380}},
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
