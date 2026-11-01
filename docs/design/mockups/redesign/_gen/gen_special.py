import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
src = open(os.path.join(HERE, "gen_chatact.py"), encoding="utf-8").read()
# Общие куски (CSS, иконки, page, шапка, поле ввода) — из генератора «Действий в чате».
exec(src[:src.index("# Сообщения ленты по умолчанию")].replace("OUT = sys.argv[1]", "OUT = sys.argv[1]"))
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

EXTRA = """.st{display:inline-flex;align-items:center;vertical-align:-2px;margin-left:3px}
.fail{font-size:12px;color:#FF8A8A;align-self:flex-end;display:flex;align-items:center;gap:6px;margin:2px 4px 0 0}
.fail b{font-weight:500;color:#7FD0EE}
.del{align-self:flex-start;background:#1B1C20;color:#8E8E98;font-size:14px;font-style:italic;padding:7px 12px;border-radius:18px;display:flex;align-items:center;gap:6px;box-shadow:inset 0 0 0 1px #26272D}
.fw{font-size:13px;color:#7FD0EE;font-weight:500;display:flex;align-items:center;gap:4px;margin-bottom:2px}
.sys{align-self:center;max-width:300px;text-align:center;font-size:13px;line-height:18px;color:#9AA3AD;padding:4px 10px;margin:4px 0}
.sys b{color:#C4C7CC;font-weight:500}
.call{display:flex;align-items:center;gap:12px;padding:10px 10px 10px 12px;min-width:220px}
.cic{width:36px;height:36px;border-radius:50%;display:flex;align-items:center;justify-content:center;flex:none}
.ct{flex:1;min-width:0}
.ct div:first-child{font-size:15px;color:#ECEDEF}
.ct div:last-child{font-size:12px;color:#9AA3AD;margin-top:1px;font-variant-numeric:tabular-nums}
.cb{width:36px;height:36px;border-radius:50%;border:none;background:rgba(255,255,255,0.08);display:flex;align-items:center;justify-content:center;flex:none}
.thumb{width:230px;height:160px;border-radius:18px;position:relative;box-shadow:inset 0 0 0 1px rgba(255,255,255,0.1);display:flex;align-items:center;justify-content:center;overflow:hidden}
.dl{width:48px;height:48px;border-radius:50%;background:rgba(0,0,0,0.55);border:none;display:flex;align-items:center;justify-content:center}
.sz{position:absolute;left:8px;bottom:8px;font-size:11px;color:#FFFFFF;background:rgba(0,0,0,0.55);padding:2px 6px;border-radius:8px;font-variant-numeric:tabular-nums}
.tm{position:absolute;right:8px;bottom:8px;font-size:11px;color:#FFFFFF;background:rgba(0,0,0,0.55);padding:2px 6px;border-radius:8px}
.file{display:flex;align-items:center;gap:12px;padding:10px 12px;min-width:240px;max-width:290px;box-sizing:border-box}
.fic{width:44px;height:44px;border-radius:12px;background:#33B5E5;display:flex;align-items:center;justify-content:center;flex:none;position:relative}
.fn{flex:1;min-width:0}
.fn div:first-child{font-size:15px;color:#ECEDEF;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.fn div:last-child{font-size:12px;color:#9AA3AD;margin-top:2px;font-variant-numeric:tabular-nums}
.warn{display:flex;gap:10px;align-items:flex-start;background:#33260F;border-radius:14px;padding:12px;color:#FFCC80;font-size:14px;line-height:19px}
.card{background:#1C1D22;border-radius:18px;padding:18px 16px;display:flex;flex-direction:column;align-items:center;gap:6px;text-align:center}
.unr{display:flex;align-items:center;gap:10px;margin:8px -14px;padding:0 14px}
.unr i{flex:1;height:1px;background:#2E4B57}
.unr span{font-size:12px;font-weight:500;color:#7FD0EE}
.fab{position:absolute;right:16px;width:46px;height:46px;border-radius:50%;background:#26272D;border:none;display:flex;align-items:center;justify-content:center;box-shadow:0 6px 18px rgba(0,0,0,0.45),0 0 0 1px rgba(255,255,255,0.05)}
.bdg{position:absolute;top:-6px;left:50%;transform:translateX(-50%);min-width:20px;height:20px;border-radius:10px;background:#33B5E5;color:#04222E;font-size:12px;font-weight:700;display:flex;align-items:center;justify-content:center;padding:0 5px;box-sizing:border-box}
"""
CSS += EXTRA

CLOCK = '<circle cx="12" cy="12" r="8"></circle><path d="M12 8v4l2.5 2"></path>'
CLOUD = '<path d="M7 18h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6 11a3.5 3.5 0 0 0 1 7z"></path>'
ALERT = '<circle cx="12" cy="12" r="9"></circle><path d="M12 7.5v5.5M12 16.5h.01"></path>'
BAN = '<circle cx="12" cy="12" r="8"></circle><path d="M6.5 17.5l11-11"></path>'
PHONE = '<path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2"></path>'
VIDEO = '<rect x="3" y="7" width="12" height="10" rx="2"></rect><path d="M15 10.5l6-3.5v10l-6-3.5"></path>'
DOWNL = '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"></path>'
WARN_T = '<path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17h.01"></path>'
KEY = '<circle cx="8" cy="15" r="4"></circle><path d="M11 12l9-9M17 6l3 3M14 9l2 2"></path>'
CHEVD = '<path d="M7 10l5 5 5-5"></path>'
AT = '<circle cx="12" cy="12" r="4"></circle><path d="M16 12v1.5a2.5 2.5 0 0 0 5 0V12a9 9 0 1 0-3.5 7.1"></path>'
SENT1 = ('<svg width="12" height="10" viewBox="0 0 12 12" fill="none" stroke="#9AD7EE" stroke-width="1.8" stroke-linecap="round" '
         'stroke-linejoin="round" aria-label="Отправлено" role="img"><path d="M1.5 6.5l3.2 3.2L10.5 2.5"></path></svg>')


def stico(path, label):
    return ('<span class="st"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#9FC6D6" stroke-width="1.8" '
            f'stroke-linecap="round" stroke-linejoin="round" role="img" aria-label="{label}">{path}</svg></span>')


def out(text, time, mark, cls="bout"):
    return f'<div class="{cls}">{text}<span class="mto">{time} {mark}</span></div>'


def wrapcol(inner, side="end"):
    return f'<div style="align-self:flex-{side};display:flex;flex-direction:column;align-items:flex-{side};">{inner}</div>'


LBL = lambda t: f'<div class="lbl">{t}</div>'

# ---------- 1. Статусы отправки ----------
s_feed = (FEED_OPEN + LBL("Сегодня")
          + out("Скинь адрес, пожалуйста", "14:02", TICKS)
          + out("Буду к восьми", "14:03", SENT1)
          + out("И захвати зарядку", "14:05", stico(CLOUD, "В очереди: собеседник не в сети"))
          + wrapcol('<div class="bout" style="box-shadow:inset 0 0 0 1px #6B2A2E;">Видео с прошлого раза, 214 МБ'
                    f'<span class="mto">14:06 <span class="st">{ic(ALERT, 13, "#FF8A8A")}</span></span></div>'
                    '<div class="fail">Не доставлено · <b>Повторить</b></div>')
          + out("ок", "14:07", stico(CLOCK, "Отправляется"))
          + '</div>')
statuses = page("Особые: статусы отправки", chat_header(status="не в сети") + PANEL + s_feed + composer() + '</div>')

# ---------- 2. Удалённое, изменённое, пересланное ----------
d_feed = (FEED_OPEN + LBL("Сегодня")
          + f'<div class="del">{ic(BAN, 15, "#8E8E98")}Сообщение удалено</div>'
          + '<div class="bin s2">Перепутала чат, не обращай внимания<span class="mt">изменено · 13:41</span></div>'
          + SP
          + f'<div class="bout"><div class="fw">{ic(FWD, 14, "#7FD0EE")}Переслано от Бориса</div>Ключ под горшком у крыльца, справа<span class="mto">13:45 {TICKS}</span></div>'
          + f'<div class="bout"><div class="fw">{ic(FWD, 14, "#7FD0EE")}Переслано</div>Код домофона 2580<span class="mto">13:45 {TICKS}</span></div>'
          + SP
          + '<div class="bin s1"><div class="q" style="border-left-color:#6B6C74;"><div style="color:#9AA3AD;font-weight:500;">Вы</div><i style="color:#8E8E98;">сообщение удалено</i></div>Ага, поняла<span class="mt">13:50</span></div>'
          + '</div>')
deleted = page("Особые: удалённое и пересланное", chat_header() + PANEL + d_feed + composer() + '</div>')

# ---------- 3. Системные строки группы ----------
g_feed = (FEED_OPEN + LBL("3 октября")
          + '<div class="sys">Группа создана · <b>Борис</b></div>'
          + '<div class="sys"><b>Кирилл</b> в группе · приглашение от <b>Бориса</b></div>'
          + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#4DB6AC;margin-bottom:2px;">Борис</div>Все в сборе<span class="mt">10:01</span></div>'
          + LBL("Вчера")
          + '<div class="sys"><b>Мария</b> больше не в группе</div>'
          + '<div class="sys">Новое название: <b>«Дача 2026»</b></div>'
          + '<div class="sys"><b>Кирилл</b> — теперь администратор</div>'
          + LBL("Сегодня")
          + '<div class="sys"><b>Мария</b> снова в группе · приглашение от <b>Кирилла</b></div>'
          + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#CE93D8;margin-bottom:2px;">Мария</div>Вернулась. Что пропустила?<span class="mt">12:20</span></div>'
          + '</div>')
gsys = page("Особые: системные строки группы", chat_header("Дача 2026", "4 участника", "#5D4037", sq=True) + PANEL + g_feed + composer() + '</div>')


# ---------- 4. Звонки в ленте ----------
def call(side, icon, icolor, ibg, title, sub, cls):
    return (f'<div class="{cls}" style="padding:0;"><div class="call"><div class="cic" style="background:{ibg};">{ic(icon, 18, icolor)}</div>'
            f'<div class="ct"><div>{title}</div><div>{sub}</div></div>'
            f'<button class="cb" aria-label="Позвонить">{ic(PHONE, 16, "#E6E7EA")}</button></div></div>')


c_feed = (FEED_OPEN + LBL("Вчера")
          + call("in", PHONE, "#FF8A8A", "#3A1E22", "Пропущенный звонок", "21:14", "bin")
          + call("in", PHONE, "#FF8A8A", "#3A1E22", "Пропущенный звонок", "21:16", "bin s2")
          + SP
          + call("out", PHONE, "#7FD0EE", "#13313D", "Исходящий звонок", "12:40 · 21:30", "bout")
          + LBL("Сегодня")
          + call("in", VIDEO, "#A5D6A7", "#1E3322", "Входящий видеозвонок", "3:12 · 09:05", "bin")
          + SP
          + call("out", PHONE, "#9AA3AD", "#26272D", "Исходящий · без ответа", "11:02", "bout")
          + '</div>')
calls = page("Особые: звонки в ленте", chat_header() + PANEL + c_feed + composer() + '</div>')

# ---------- 5. Запрос от незнакомца (7e) ----------
req_head = ('<div style="display: flex; align-items: center; gap: 10px; padding: 14px 12px 14px 8px;">'
            + btn('<path d="M15 5l-7 7 7 7"></path>', "Назад")
            + '<div style="width: 40px; height: 40px; border-radius: 50%; background: #455A64; color: #FFFFFF; font-size: 16px; font-weight: 500; display: flex; align-items: center; justify-content: center; flex: none;">А</div>'
            + '<div style="flex: 1; min-width: 0; display: flex; flex-direction: column;"><div style="font-size: 17px; font-weight: 500; color: #ECEDEF;">Алиса</div>'
            + '<div style="font-size: 12px; color: #8E8E98;">запрос в контакты</div></div></div>')
request = page("Особые: запрос от незнакомца", req_head
               + '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; gap: 12px; padding: 10px; overflow: hidden;">'
               + '<div class="card"><div style="width:64px;height:64px;border-radius:50%;background:#455A64;color:#FFF;font-size:26px;font-weight:500;display:flex;align-items:center;justify-content:center;margin-bottom:4px;">А</div>'
               + '<div style="font-size:20px;font-weight:700;color:#ECEDEF;">Алиса</div>'
               + '<div class="sub">Имя указано отправителем</div>'
               + f'<div style="display:flex;align-items:center;gap:6px;margin-top:6px;font-size:13px;color:#9AA3AD;font-family:ui-monospace,monospace;">{ic(KEY, 15, "#9AA3AD")}3F9A 7C21 · E04B 11D8</div></div>'
               + f'<div class="warn">{ic(WARN_T, 20, "#FFB74D", NF)}<div>Имя совпадает с контактом <b style="font-weight:500;">Алиса</b>, ключ другой. Это может быть другой человек.<div style="margin-top:6px;"><a href="#" style="color:#FFCC80;font-weight:500;">Сверить ключ лично</a></div></div></div>'
               + '<div class="hd" style="font-size:13px;font-weight:500;color:#8E8E98;padding:4px 6px 0;">Первое сообщение</div>'
               + '<div class="bin s1" style="margin:0 4px;">Привет, это я, телефон новый. Скинешь код от домофона?<span class="mt">14:31</span></div>'
               + '<div style="flex:1;"></div>'
               + '<div style="display:flex;flex-direction:column;gap:8px;padding:0 4px 8px;">'
               + '<button class="btn pri">Добавить в контакты</button>'
               + '<div style="display:flex;gap:8px;"><button class="btn">Отклонить</button><button class="btn red">Заблокировать</button></div></div>'
               + '</div>')

# ---------- 6. Приглашение в группу ----------
invite = page("Особые: приглашение в группу", req_head.replace("Алиса</div>", "Дача 2026</div>").replace(">А</div>", ">Д</div>")
              .replace("border-radius: 50%; background: #455A64", "border-radius: 14px; background: #5D4037").replace("запрос в контакты", "приглашение в группу")
              + '<div style="flex: 1; min-height: 0; background: #16171B; border-radius: 28px 28px 0 0; display: flex; flex-direction: column; gap: 12px; padding: 10px; overflow: hidden;">'
              + '<div class="card"><div style="width:64px;height:64px;border-radius:20px;background:#5D4037;color:#FFF;font-size:26px;font-weight:500;display:flex;align-items:center;justify-content:center;margin-bottom:4px;">Д</div>'
              + '<div style="font-size:20px;font-weight:700;color:#ECEDEF;">Дача 2026</div>'
              + '<div class="sub">4 участника · приглашение от Бориса</div>'
              + '<div style="display:flex;margin-top:10px;">'
              + ''.join(f'<div style="width:32px;height:32px;border-radius:50%;background:{c};color:#FFF;font-size:13px;font-weight:500;display:flex;align-items:center;justify-content:center;margin-left:{m}px;box-shadow:0 0 0 2px #1C1D22;">{a}</div>'
                        for a, c, m in [("Б", "#00897B", 0), ("М", "#8E24AA", -8), ("К", "#C2185B", -8), ("Б", "#6D4C41", -8)])
              + '</div></div>'
              + '<div class="sub" style="padding:0 6px;">Переписка до вступления не видна: сообщения начнут приходить после «Вступить».</div>'
              + '<div style="flex:1;"></div>'
              + '<div style="display:flex;gap:8px;padding:0 4px 8px;"><button class="btn">Отклонить</button><button class="btn pri">Вступить</button></div>'
              + '</div>')

# ---------- 7. Вложения не скачаны ----------
ring = ('<svg width="44" height="44" viewBox="0 0 44 44" aria-label="Скачано 34%" role="img"><circle cx="22" cy="22" r="19" fill="none" stroke="rgba(255,255,255,0.25)" stroke-width="3"></circle>'
        '<circle cx="22" cy="22" r="19" fill="none" stroke="#FFFFFF" stroke-width="3" stroke-linecap="round" stroke-dasharray="40.6 119.4" transform="rotate(-90 22 22)"></circle></svg>')
a_feed = (FEED_OPEN + LBL("Сегодня")
          + '<div class="thumb s1" style="align-self:flex-start;background:linear-gradient(135deg,#3F5546,#6E8A73);filter:saturate(0.8);">'
          + f'<button class="dl" aria-label="Скачать фото">{ic(DOWNL, 22, "#FFFFFF")}</button><span class="sz">2,4 МБ</span><span class="tm">10:12</span></div>'
          + '<div class="thumb s2" style="align-self:flex-start;background:linear-gradient(135deg,#3D3550,#6B5E86);height:140px;">'
          + f'<button class="dl" aria-label="Скачать видео">{ic(DOWNL, 22, "#FFFFFF")}</button><span class="sz">0:48 · 18 МБ</span><span class="tm">10:13</span></div>'
          + SP
          + f'<div class="bin s1" style="padding:0;"><div class="file"><div class="fic">{ic(DOWNL, 20, "#04222E")}</div><div class="fn"><div>договор аренды.pdf</div><div>12 МБ · PDF · 10:20</div></div></div></div>'
          + f'<div class="bin s2" style="padding:0;"><div class="file"><div class="fic" style="background:#26272D;">{ring}<span style="position:absolute;inset:0;display:flex;align-items:center;justify-content:center;">{ic(CLOSE, 14, "#FFFFFF")}</span></div><div class="fn"><div>план участка.dwg</div><div>4,1 из 12 МБ</div></div></div></div>'
          + f'<div class="bin s2" style="padding:0;"><div class="file"><div class="fic" style="background:#26272D;">{ic(CLOCK, 20, "#9AA3AD")}</div><div class="fn"><div>фото с дачи.zip</div><div style="white-space:normal;">340 МБ · скачается, когда Алиса будет в сети</div></div></div></div>'
          + '</div>')
attach = page("Особые: вложения не скачаны", chat_header(status="не в сети") + PANEL + a_feed + composer() + '</div>')

# ---------- 8. Непрочитанные ----------
u_feed = (FEED_OPEN + LBL("Сегодня")
          + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#4DB6AC;margin-bottom:2px;">Борис</div>Кто в субботу едет?<span class="mt">19:02</span></div>'
          + SP + out("Я, с утра", "19:05", TICKS)
          + '<div class="unr"><i></i><span>Непрочитанные · 5</span><i></i></div>'
          + '<div class="bin s1"><div style="font-size:13px;font-weight:500;color:#CE93D8;margin-bottom:2px;">Мария</div>Я после обеда<span class="mt">19:40</span></div>'
          + '<div class="bin s1" style="margin-top:6px;"><div style="font-size:13px;font-weight:500;color:#F48FB1;margin-bottom:2px;">Кирилл</div>Могу подвезти двоих<span class="mt">19:44</span></div>'
          + '</div>')
unread = page("Особые: непрочитанные", chat_header("Дача 2026", "4 участника", "#5D4037", sq=True) + PANEL + u_feed + composer() + '</div>'
              + f'<button class="fab" style="bottom:140px;" aria-label="К упоминанию">{ic(AT, 20)}<span class="bdg">1</span></button>'
              + f'<button class="fab" style="bottom:84px;" aria-label="Вниз, непрочитанных: 3">{ic(CHEVD, 22)}<span class="bdg">3</span></button>')

rows = [
    [("Main.dc.html", statuses, "Статусы: ✓✓ · ✓ · в очереди · не доставлено · отправляется"),
     ("Deleted.dc.html", deleted, "Удалённое, изменённое, пересланное"),
     ("GroupSystem.dc.html", gsys, "Системные строки группы — без рода"),
     ("Calls.dc.html", calls, "Звонки в ленте")],
    [("Request.dc.html", request, "Запрос от незнакомца + совпадение имени (7e)"),
     ("Invite.dc.html", invite, "Приглашение в группу"),
     ("Attach.dc.html", attach, "Вложения: не скачано · качается · ждёт отправителя"),
     ("Unread.dc.html", unread, "Непрочитанные: черта, «вниз», «@»")],
]
row_titles = ["Особые сообщения в ленте", "Запросы, приглашения, вложения, непрочитанное"]
boards, order, notes = {}, [], {}
for ri, r in enumerate(rows):
    y = ri * 1235
    for ci, (name, html, title) in enumerate(r):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(html)
        boards[name] = {"x": ci * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
    notes[f"t{ri + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[ri], "kind": "title1", "maxW": len(r) * 492 - 80}
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T21:00:00Z"}, "title": "Особые сообщения Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes, "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok", len(order))
