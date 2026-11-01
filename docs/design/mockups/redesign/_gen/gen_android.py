import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
pp = open(os.path.join(HERE, "gen_people.py"), encoding="utf-8").read()
# Общее (CSS, иконки, page, шапка, панель «листа», строки людей) — из генератора контактов.
exec(pp[:pp.index("CORNERS = (")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

C["С"] = "#2B5F73"
CSS += """.thumb{width:76px;height:76px;border-radius:14px;flex:none}
.warn{display:flex;gap:12px;padding:14px 16px;border-radius:18px;background:#2A2116;color:#F2C98A;font-size:14px;line-height:20px}
.warn a{color:#FFCC80;font-weight:500;text-decoration:none}
.lnk{font-size:15px;line-height:21px;color:#ECEDEF;word-break:break-all}
.lnk.u{color:#7FD0EE;text-decoration:underline}
.bub{align-self:flex-start;max-width:300px;background:#26272D;border-radius:18px 18px 18px 6px;padding:10px 14px;display:flex;flex-direction:column;gap:6px}
.sys{background:#2A2B31;border-radius:28px 28px 0 0;padding:12px 16px 24px;display:flex;flex-direction:column;gap:14px}
.sys .tl{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px}
.sys .ti{display:flex;flex-direction:column;align-items:center;gap:6px;font-size:12px;color:#ECEDEF;text-align:center}
.sys .ti .av{width:56px;height:56px;font-size:22px;position:relative}
.sys .ti small{font-size:11px;color:#9AA3AD}
.badge{position:absolute;right:-2px;bottom:-2px;width:20px;height:20px;border-radius:6px;background:#33B5E5;border:2px solid #2A2B31;box-sizing:border-box}
"""

WARN = '<path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17h.01"></path>'
LINK = '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"></path><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"></path>'


def xhdr(title, right=""):
    return '<div class="hdr">' + btn(CLOSE, "Закрыть") + f'<b>{title}</b>{right}</div>'


# ---------- 1. «Поделиться» из другого приложения → выбор чата ----------
thumbs = ('<div style="display:flex;gap:8px;align-items:center;padding-bottom:16px;">'
          '<div class="thumb" role="img" aria-label="Фото 1" style="background:linear-gradient(135deg,#6D8A96,#2E3D45);"></div>'
          '<div class="thumb" role="img" aria-label="Фото 2" style="background:linear-gradient(135deg,#A78B6B,#4A3B2C);"></div>'
          '<div class="cap" style="padding:0 0 0 6px;">2 фото · 4,1 МБ</div></div>')
share_in = page("Поделиться в Peerage: выбор чата", xhdr("Отправить в Peerage") + body(
    thumbs
    + f'<div class="srch">{ic(SEARCH, 20, "#7A7A84", NF)}Кому</div>' + '<div style="height:12px;"></div>'
    + '<div class="grp">'
    + prow("Сохранённое", "заметки себе", check(False))
    + prow("Алиса", "в сети", check(True))
    + prow("Дача", "группа · 6 участников", check(True)).replace('border-radius:50%', 'border-radius:14px', 1)
    + prow("Борис", "в сети вчера", check(False))
    + prow("Вера", "в сети", check(False))
    + '</div>' + SP16
    + '<div class="inp"><span class="p">Подпись</span></div>'
    + FILL) + foot("Отправить в 2 чата"))

# ---------- 2. Системное меню «Поделиться»: последние чаты ----------
def tile(name, app="Peerage", sq=False):
    r = "18px" if sq else "50%"
    return (f'<div class="ti"><div class="av" style="background:{C[name[0]]};border-radius:{r};">{name[0]}'
            f'<i class="badge"></i></div>{name}<small>{app}</small></div>')


sys_sheet = page("Системное меню «Поделиться»", '<div style="flex:1;background:#0F1012;"></div>'
                 + '<div class="sys" role="dialog" aria-label="Системное меню">'
                 + '<div class="cap" style="padding:0;">Набросок системного меню Android: внешний вид задаёт система</div>'
                 + '<div class="tl">' + tile("Алиса") + tile("Дача", sq=True) + tile("Борис") + tile("Вера") + '</div>'
                 + '<div style="height:1px;background:#3A3B42;"></div>'
                 + '<div class="tl">'
                 + ''.join(f'<div class="ti"><div class="av" style="background:#3A3B42;border-radius:16px;"></div>{t}</div>'
                           for t in ["Peerage", "Почта", "Диск", "Ещё"])
                 + '</div></div>')

# ---------- 3. Ссылка · А · https со своим доменом ----------
def foreign(link_html, note):
    return ('<div class="hdr"><div style="width:40px;"></div><b style="color:#9AA3AD;font-weight:500;font-size:15px;">'
            'Другой мессенджер</b></div>'
            + body('<div style="display:flex;flex-direction:column;gap:10px;flex:1;justify-content:flex-end;">'
                   + '<div class="bub"><div class="lnk">Добавляйся в Peerage:</div>' + link_html + '</div>'
                   + f'<div class="cap" style="padding:8px 4px 0;">{note}</div></div>'))


link_a = page("Ссылка-приглашение: А, https", foreign(
    '<a class="lnk u" href="#">https://[домен]/add#3f9a1c07e2b4…</a>',
    'Ссылка нажимается в любом мессенджере. Нажатие открывает Peerage, без приложения — страницу с ключом '
    'и ссылкой на установку. Часть после # браузер на сервер не отправляет.'))

# ---------- 4. Ссылка · Б · своя схема ----------
link_b = page("Ссылка-приглашение: Б, своя схема", foreign(
    '<div class="lnk">peerage:add/3f9a1c07e2b4…</div>',
    'Домен не нужен. Большинство мессенджеров такую ссылку не делают нажимаемой: её копируют и вставляют '
    'в «Добавить контакт → Вставить код».'))

# ---------- 5. Мой код: ссылка ----------
my = page("Мой код: ссылка", hdr("Мой код") + body(
    '<div style="display:flex;flex-direction:column;align-items:center;gap:14px;padding:8px 0 20px;">'
    + qr_svg() + '<div class="obt" style="margin:0;font-size:22px;">Аня</div></div>'
    + '<div class="grp">'
    + grow(LINK, "Ссылка-приглашение", "https://[домен]/add#3f9a1c07e2b4…", '')
    + grow(COPY, "Скопировать ссылку", None, '')
    + grow(SHARE, "Поделиться", None, '')
    + '</div>'
    + '<div class="cap" style="margin-top:8px;">Ссылка и QR — одно и то же: ключ, адрес и имя. По ссылке приходит обычный запрос в контакты.</div>'
    + FILL))

# ---------- 6. Открыта ссылка ----------
opened = page("Открыта ссылка-приглашение", hdr("Добавить контакт") + body(
    '<div style="display:flex;align-items:center;gap:14px;padding:8px 0 20px;">'
    + av("Алиса", 64) + '<div><div class="obt" style="margin:0;font-size:22px;">Алиса</div>'
    + '<div class="cap" style="font-variant-numeric:tabular-nums;padding:0;">ключ 3f9a1c07… · из ссылки</div></div></div>'
    + '<div class="sec">Приветствие</div>'
    + '<div class="inp" style="align-items:flex-start;min-height:96px;padding-top:14px;"><span class="p" style="line-height:22px;">Например: кто это и откуда</span></div>'
    + '<div class="cap" style="margin-top:8px;">Уйдёт запросом в контакты вместе с именем этого профиля; фото — после принятия.</div>'
    + FILL) + foot("Отправить запрос", "Отмена"))

# ---------- 7. Запрос с именем как у контакта ----------
req = page("Запрос: имя как у контакта", hdr("Запрос в контакты") + body(
    '<div style="display:flex;align-items:center;gap:14px;padding:8px 0 16px;">'
    + av("Алиса", 64) + '<div><div class="obt" style="margin:0;font-size:22px;">Алиса</div>'
    + '<div class="cap" style="font-variant-numeric:tabular-nums;padding:0;">ключ 7c219b0e… · сегодня 14:31</div></div></div>'
    + f'<div class="warn">{ic(WARN, 20, "#FFB74D", NF)}<div>Имя совпадает с контактом <b style="font-weight:500;">Алиса</b> '
      '(ключ 3f9a1c07…), ключ другой.<div style="margin-top:6px;"><a href="#">Сверить ключ лично</a></div></div></div>'
    + SP16 + '<div class="sec">Приветствие</div>'
    + '<div class="bub" style="max-width:none;align-self:stretch;"><div class="lnk">Привет, это я, новый телефон. Старый разбился</div></div>'
    + FILL) + foot("Принять", "Отклонить", '<button class="btn ghost" style="color:#FF8A8A;">Заблокировать</button>'))

# ---------- 8. Список запросов: строка с предупреждением ----------
WI = f'<span role="img" aria-label="Имя совпадает с контактом" style="display:inline-flex;">{ic(WARN, 16, "#FFB74D", NF)}</span>'
reqs = page("Запросы: имя как у контакта", hdr("Запросы") + body(
    '<div class="grp">'
    + prow("Алиса", f'{WI} имя как у контакта · «Привет, это я, новый…»', '<span class="tag">открыть</span>')
    + prow("Глеб", "«Добрый день, по поводу дачи»", '<span class="tag">принять</span>')
    + '</div>'
    + '<div class="cap" style="margin-top:8px;">Запрос с совпадающим именем принимается только из карточки.</div>'
    + FILL))

rows = [
    [("Main.dc.html", share_in, "«Поделиться»: выбор чата"),
     ("SystemShare.dc.html", sys_sheet, "Системное меню: последние чаты")],
    [("LinkA.dc.html", link_a, "Ссылка · А: https и свой домен"),
     ("LinkB.dc.html", link_b, "Ссылка · Б: своя схема"),
     ("MyCode.dc.html", my, "Мой код: ссылка"),
     ("LinkOpened.dc.html", opened, "Открыта ссылка → запрос")],
    [("Request.dc.html", req, "Запрос: имя как у контакта"),
     ("Requests.dc.html", reqs, "Список запросов")],
]
row_titles = ["Поделиться в Peerage", "Ссылка-приглашение", "Имя как у контакта"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 492 * len(row) - 80}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(close_panel(html))
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T22:40:00Z"}, "title": "Встраивание в Android: Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
