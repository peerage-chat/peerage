import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
src = open(os.path.join(HERE, "gen_chatact.py"), encoding="utf-8").read()
# Общие куски (CSS, иконки, page) — из генератора «Действий в чате».
exec(src[:src.index("# ---------- 1. А · Панель вместо шапки ----------")])
P = os.path.join(OUT, "project")
os.makedirs(P, exist_ok=True)

CSS += """.ob{flex:1;min-height:0;display:flex;flex-direction:column;padding:0 20px}
.obt{font-size:28px;line-height:34px;font-weight:700;color:#ECEDEF;margin:8px 0 8px}
.obs{font-size:15px;line-height:21px;color:#8E8E98}
.obf{display:flex;flex-direction:column;gap:8px;padding:12px 20px 24px}
.btn.ghost{background:none;color:#33B5E5}
.top{display:flex;align-items:center;gap:8px;padding:14px 12px 10px 8px;min-height:40px}
.prog{flex:1;display:flex;gap:6px;padding-right:12px}
.prog i{flex:1;height:4px;border-radius:2px;background:#26272D}
.prog i.on{background:#33B5E5}
.grp{background:#1C1D22;border-radius:18px;display:flex;flex-direction:column;overflow:hidden}
.gr{display:flex;align-items:center;gap:14px;min-height:56px;padding:10px 16px;box-sizing:border-box}
.gr + .gr{box-shadow:inset 0 1px 0 #26272D}
.gr b{display:block;font-size:15px;font-weight:500;color:#ECEDEF}
.gr span{display:block;font-size:13px;line-height:18px;color:#8E8E98;margin-top:2px}
.gt{flex:1;min-width:0}
.pill{min-height:36px;padding:0 14px;border-radius:18px;border:none;background:#13313D;color:#7FD0EE;font-family:inherit;font-size:14px;font-weight:500;flex:none}
.ok{display:flex;align-items:center;gap:6px;font-size:14px;color:#8BC34A;flex:none}
.warn{display:flex;gap:12px;padding:14px 16px;border-radius:18px;background:#2A2116;color:#F2C98A;font-size:14px;line-height:20px}
.inp{display:flex;align-items:center;gap:12px;min-height:52px;padding:0 16px;border-radius:16px;background:#1C1D22;box-sizing:border-box}
.inp .v{flex:1;font-size:16px;color:#ECEDEF}
.inp .p{flex:1;font-size:16px;color:#7A7A84}
.cap{font-size:13px;line-height:18px;color:#8E8E98;padding:0 4px}
.wg{display:grid;grid-template-columns:1fr 1fr;grid-auto-flow:column;grid-template-rows:repeat(12,auto);column-gap:12px;background:#1C1D22;border-radius:18px;padding:10px 16px}
.w{display:flex;align-items:baseline;gap:10px;min-height:36px;align-items:center;font-size:16px;font-weight:500;color:#ECEDEF}
.w i{font-style:normal;width:20px;text-align:right;font-size:12px;font-weight:400;color:#7A7A84;font-variant-numeric:tabular-nums}
.w u{text-decoration:none;flex:1;min-width:0;align-self:stretch;display:flex;align-items:center;border-bottom:1px solid #2E2F36}
.w u.on{border-bottom:1.5px solid #33B5E5}
.w u.bad{border-bottom:1.5px solid #FF6B6B;color:#FF8A8A}
.kb{background:#1A1B1F;padding:6px 4px 10px;display:flex;flex-direction:column;gap:8px}
.sug{display:flex;gap:4px;padding:2px 4px}
.sug button{flex:1;min-height:40px;border:none;border-radius:10px;background:#26272D;color:#ECEDEF;font-family:inherit;font-size:15px}
.sug button.b{background:#13313D;color:#7FD0EE}
.kr{display:flex;justify-content:center;gap:5px}
.k{width:34px;height:42px;border-radius:6px;background:#2C2D33;color:#D8D9DC;font-size:17px;display:flex;align-items:center;justify-content:center}
.k.w2{width:52px;font-size:13px}.k.sp{width:180px}
.qz{display:flex;flex-direction:column;gap:8px}
.qh{font-size:14px;color:#B8B8C0;padding:0 4px}
.qo{display:flex;gap:6px}
.qo button{flex:1;min-height:44px;border:none;border-radius:14px;background:#1C1D22;color:#ECEDEF;font-family:inherit;font-size:15px}
.qo button.on{background:#13313D;color:#7FD0EE;box-shadow:inset 0 0 0 1.5px #33B5E5}
.stat{display:flex;justify-content:space-between;align-items:center;min-height:44px;padding:0 16px;font-size:15px;color:#ECEDEF}
.stat + .stat{box-shadow:inset 0 1px 0 #26272D}
.stat span{color:#8E8E98;font-variant-numeric:tabular-nums}
.big{width:72px;height:72px;border-radius:24px;background:#13313D;display:flex;align-items:center;justify-content:center;margin:0 0 12px}
"""

BACK = '<path d="M15 5l-7 7 7 7"></path>'
BELL = '<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z"></path><path d="M10 20a2 2 0 0 0 4 0"></path>'
BATT = '<rect x="3" y="7" width="16" height="10" rx="2"></rect><path d="M21 10.5v3"></path><path d="M11.5 9l-2 3.5h3l-2 3.5"></path>'
ROCKET = '<path d="M12 3c3 2 5 5.5 5 9.5L15 17H9l-2-4.5C7 8.5 9 5 12 3z"></path><circle cx="12" cy="10" r="1.8"></circle><path d="M9 17l-1.5 3M15 17l1.5 3"></path>'
KEY = '<circle cx="8" cy="15" r="4"></circle><path d="M11 12l8-8M16 7l2 2M14 9l2 2"></path>'
SHIELD = '<path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z"></path><path d="M9 12l2 2 4-4"></path>'
WARN = '<path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17h.01"></path>'
USER = '<circle cx="12" cy="8" r="4"></circle><path d="M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6"></path>'
PHONE_OFF = '<path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2"></path><path d="M3 3l18 18"></path>'
BOOK_OFF = '<path d="M7 4h11a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"></path><circle cx="12" cy="10" r="2.2"></circle><path d="M8.5 16a3.5 3.5 0 0 1 7 0"></path><path d="M3 3l18 18"></path>'
PIN_OFF = '<path d="M12 21s-6-5.5-6-11a6 6 0 0 1 12 0c0 5.5-6 11-6 11z"></path><circle cx="12" cy="10" r="2"></circle><path d="M3 3l18 18"></path>'
SERVER_OFF = '<rect x="4" y="4" width="16" height="6" rx="2"></rect><rect x="4" y="14" width="16" height="6" rx="2"></rect><path d="M8 7h.01M8 17h.01"></path><path d="M3 3l18 18"></path>'
PASTE = '<rect x="7" y="4" width="10" height="4" rx="1"></rect><path d="M7 6H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-1"></path>'
ARCHIVE = '<rect x="3" y="4" width="18" height="4" rx="1"></rect><path d="M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8M10 12h4"></path>'
EYE_OFF = '<path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6z"></path><circle cx="12" cy="12" r="2.5"></circle><path d="M4 4l16 16"></path>'
MESH = ('<svg width="72" height="72" viewBox="0 0 72 72" fill="none" stroke="#33B5E5" stroke-width="2" aria-hidden="true">'
        '<path d="M18 22L52 18L56 50L22 54Z M18 22L56 50 M52 18L22 54" stroke-opacity="0.45"></path>'
        '<circle cx="18" cy="22" r="6" fill="#08090B"></circle><circle cx="52" cy="18" r="6" fill="#08090B"></circle>'
        '<circle cx="56" cy="50" r="6" fill="#08090B"></circle><circle cx="22" cy="54" r="6" fill="#08090B"></circle>'
        '<circle cx="37" cy="36" r="5" fill="#33B5E5" stroke="none"></circle></svg>')

WORDS = ("orbit lunar velvet grain cradle tissue ample harbor rescue meadow quote bundle "
         "fossil drama ozone pilot timber civil oyster wealth spike gentle across reveal").split()


def top(step=None, back=True):
    prog = ''
    if step is not None:
        prog = '<div class="prog" aria-label="Шаг ' + str(step) + ' из 3">' + ''.join(
            f'<i class="{"on" if i < step else ""}"></i>' for i in range(3)) + '</div>'
    return ('<div class="top">' + (btn(BACK, "Назад") if back else '<div style="width:40px;"></div>')
            + prog + '</div>')


def foot(pri, sec=None, extra=""):
    return ('<div class="obf">' + extra + f'<button class="btn pri" style="min-height:52px;">{pri}</button>'
            + (f'<button class="btn ghost">{sec}</button>' if sec else '') + '</div>')


def screen(title, step, head, body, pri, sec=None, extra="", back=True):
    return (top(step, back) + '<div class="ob">' + head + body + '</div>' + foot(pri, sec, extra))


def heading(t, s=None, icon=None):
    h = ''
    if icon:
        h += f'<div class="big">{icon}</div>'
    h += f'<div class="obt">{t}</div>'
    if s:
        h += f'<div class="obs">{s}</div>'
    return h


def grow(icon, title, sub, right):
    return (f'<div class="gr">{ic(icon, 22, "#9AA3AD", NF)}<div class="gt"><b>{title}</b>'
            + (f'<span>{sub}</span>' if sub else '') + f'</div>{right}</div>')


SP16 = '<div style="height:16px;flex:none;"></div>'
SP24 = '<div style="height:24px;flex:none;"></div>'
FILL = '<div style="flex:1;"></div>'

# ---------- 1. Приветствие ----------
promise = ('<div class="grp">'
           + grow(PHONE_OFF, "Без номера телефона", None, '')
           + grow(BOOK_OFF, "Контакты телефона не читаются", None, '')
           + grow(PIN_OFF, "Местоположение не передаётся", None, '')
           + grow(SERVER_OFF, "Без центрального сервера", "Сообщения идут напрямую между устройствами", '')
           + '</div>')
welcome = page("Первый запуск: приветствие",
               '<div style="height:56px;flex:none;"></div><div class="ob">'
               + f'<div style="margin-bottom:20px;">{MESH}</div>'
               + '<div class="obt" style="font-size:34px;line-height:40px;">Peerage</div>'
               + '<div class="obs">Переписка и звонки без серверов. Личность — ключ на этом устройстве.</div>'
               + SP24 + promise + FILL
               + '<div class="cap" style="text-align:center;">Открытый код · GPL-3.0</div></div>'
               + foot("Создать личность", "Восстановить по 24 словам"))

# ---------- 2. Профиль ----------
avatar = ('<div style="position:relative;width:96px;height:96px;margin:28px 0 20px;">'
          + f'<div style="width:96px;height:96px;border-radius:50%;background:#26272D;display:flex;align-items:center;justify-content:center;">{ic(USER, 40, "#6B6C74")}</div>'
          + f'<div style="position:absolute;right:-2px;bottom:-2px;width:34px;height:34px;border-radius:50%;background:#33B5E5;box-shadow:0 0 0 3px #08090B;display:flex;align-items:center;justify-content:center;">{ic(CAMERA, 18, "#04222E")}</div></div>')
profile = page("Первый запуск: профиль", screen(
    "", 1, heading("Как подписаться", "Имя и фото видят контакты и те, кому передан код. Можно изменить позже."),
    avatar + '<div class="inp"><span class="v">Аня<span class="cr"></span></span></div>'
    + '<div class="cap" style="margin-top:8px;">Без имени контакты увидят короткий код ключа.</div>' + FILL,
    "Далее", "Пропустить"))

# ---------- 3. Ключ: зачем ----------
keyi = page("Первый запуск: копия ключа", screen(
    "", 2, heading("Копия ключа", "Личность — это ключ на этом телефоне. Вернуть её на новом устройстве можно только по 24 словам.", ic(KEY, 34, "#33B5E5")),
    SP24 + '<div class="grp">'
    + grow(EYE_OFF, "Записать на бумаге или в менеджер паролей", "Не фотографировать и не хранить в заметках", '')
    + grow(SHIELD, "Никому не показывать", "Кто знает слова — тот может писать от этого имени", '')
    + '</div>' + SP16
    + f'<div class="warn">{ic(WARN, 20, "#F2C98A", NF)}<div>Сервера, который хранит доступ, нет. Без 24 слов потерянный телефон — потерянная личность.</div></div>'
    + FILL,
    "Показать 24 слова", "Позже — напоминание останется на главной"))

# ---------- 4. Фраза ----------
def phrase_grid(slots):
    """Сетка 24 слов: и показ, и ввод — одна раскладка, чтобы запись на бумаге
    переносилась ячейка в ячейку. slots[i] = слово (показ) или (текст, состояние) для
    ввода: '' пусто, 'on' курсор, 'bad' нет в списке, 'ok' введено."""
    out = []
    for i, s in enumerate(slots):
        if isinstance(s, tuple):
            text, st = s
            cr = '<span class="cr"></span>' if st == "on" else ''
            cls = st if st in ("on", "bad") else ''
            s = f'<u class="{cls}">{text}{cr}</u>'
        out.append(f'<div class="w"><i>{i + 1}</i>{s}</div>')
    return '<div class="wg">' + ''.join(out) + '</div>'


grid = phrase_grid(WORDS)
phrase = page("Первый запуск: 24 слова", screen(
    "", 2, heading("24 слова", "Записать по порядку. Скриншот и запись экрана заблокированы."),
    SP16 + grid + FILL,
    "Слова записаны", None,
    extra=f'<div class="cap" style="display:flex;align-items:center;gap:6px;justify-content:center;padding-bottom:4px;">{ic(SHIELD, 16, "#8BC34A", NF)}Экран защищён от снимков</div>'))

# ---------- 5. Проверка (вариант) ----------
def keyboard(sugs):
    """Клавиатура с полосой подсказок слов BIP39 (первая — подходящая лучше всех)."""
    keys = ''.join('<div class="kr">' + ''.join(f'<div class="k">{ch}</div>' for ch in row) + '</div>'
                   for row in ["qwertyuiop", "asdfghjkl"])
    keys += '<div class="kr"><div class="k w2">⇧</div>' + ''.join(f'<div class="k">{ch}</div>' for ch in "zxcvbnm") + '<div class="k w2">⌫</div></div>'
    keys += '<div class="kr"><div class="k w2">?123</div><div class="k sp"></div><div class="k w2">далее</div></div>'
    sug = ''.join(f'<button class="{"b" if i == 0 else ""}">{w}</button>' for i, w in enumerate(sugs))
    return f'<div class="kb"><div class="sug">{sug}</div>{keys}</div>'


def word_field(n, text, on=False):
    """Поле одного слова по номеру — тот же ввод, что в сетке восстановления."""
    cr = '<span class="cr"></span>' if on else ''
    ring = ' style="box-shadow:inset 0 0 0 1.5px #33B5E5;"' if on else ''
    return (f'<div style="display:flex;flex-direction:column;gap:6px;"><div class="qh">Слово №{n}</div>'
            f'<div class="inp"{ring}><span class="v">{text}{cr}</span></div></div>')


def check_screen(title, step, sub, sec):
    """Проверка трёх слов: и в онбординге, и по напоминанию — один экран.
    Ответ один на все три («Слова не совпали»), какое неверно — не говорится."""
    return page(title, top(step) + '<div class="ob">' + heading("Проверка", sub)
                + '<div style="display:flex;flex-direction:column;gap:14px;margin-top:20px;">'
                + word_field(4, "grain") + word_field(11, "quote") + word_field(20, "wea", on=True)
                + '</div>' + FILL + '</div>' + foot("Проверить", sec)
                + keyboard(["weapon", "wealth", "weasel"]))


check = check_screen("Первый запуск: проверка слов", 2,
                     "Три слова из записанных — по номерам. Подсказка — по первым буквам.",
                     "Показать слова ещё раз")

# ---------- 6. Разрешения · А одним экраном ----------
GRANTED = f'<div class="ok">{ic(CHECK, 18, "#8BC34A")}Разрешено</div>'
perms = page("Первый запуск: разрешения", screen(
    "", 3, heading("Разрешения", "Камера, микрофон и фото запрашиваются позже — в момент, когда нужны."),
    SP24 + '<div class="grp">'
    + grow(BELL, "Уведомления", "Новые сообщения и звонки", GRANTED)
    + grow(BATT, "Работа в фоне", "Связь при погасшем экране: телефон остаётся узлом сети", '<button class="pill">Разрешить</button>')
    + grow(ROCKET, "Автозапуск", "Только Xiaomi, Huawei и похожие: без него система выгружает приложение", '<button class="pill">Открыть</button>')
    + '</div><div class="cap" style="margin-top:8px;">Без работы в фоне сообщения приходят, только пока приложение открыто.</div>' + FILL,
    "Готово"))

# ---------- 6Б. Разрешения по одному ----------
perms_b = page("Первый запуск: разрешения по одному", screen(
    "", 3, '<div style="height:40px;"></div>' + heading("Работа в фоне", None, ic(BATT, 34, "#33B5E5")),
    '<div class="obs" style="font-size:16px;line-height:23px;">Телефон остаётся узлом сети и при погасшем экране: сообщения и звонки доходят сразу, а не когда приложение открыто.</div>'
    + FILL,
    "Разрешить", "Не сейчас"))

# ---------- 7. Восстановление · ввод ----------
slots = [(w, "ok") for w in WORDS[:7]] + [("har", "on")] + [("", "")] * 16
slots[4] = ("tisue", "bad")
restore = page("Восстановление: ввод слов",
               top(None) + '<div class="ob">'
               + f'<div style="display:flex;justify-content:space-between;align-items:center;gap:12px;"><div class="obt" style="font-size:24px;line-height:30px;margin:0;">Восстановление</div><button class="pill" style="display:flex;align-items:center;gap:6px;">{ic(PASTE, 16, "#7FD0EE")}Вставить</button></div>'
               + '<div class="obs" style="margin:6px 0 4px;">24 слова в том порядке, в каком записаны.</div>'
               + '<div class="cap" style="color:#FF8A8A;padding:0 0 8px;">№5 — нет такого слова</div>'
               + phrase_grid(slots) + FILL + '</div>' + keyboard(["harbor", "hard", "harvest"]))

# ---------- 8. Восстановление · фраза не сходится ----------
swapped = [(w, "ok") for w in WORDS[:14] + ["pilot", "ozone"] + WORDS[16:]]
mismatch = page("Восстановление: фраза не сходится", screen(
    "", None, heading("Восстановление", "24 слова в том порядке, в каком записаны."),
    SP16 + phrase_grid(swapped) + SP16
    + f'<div class="warn" style="background:#2E1A1D;color:#FFB4B4;">{ic(WARN, 20, "#FF8A8A", NF)}<div>Все слова из списка, но фраза не сходится. Чаще всего два слова переставлены местами или одно записано с ошибкой.</div></div>'
    + FILL, "Восстановить"))

# ---------- 9. Личность восстановлена → переписка ----------
restored = page("Восстановление: личность на месте", screen(
    "", None, '<div style="height:24px;"></div>' + heading("Личность восстановлена", "Контакты узнают это устройство по тому же ключу.", ic(CHECK, 34, "#8BC34A")),
    SP24 + '<div class="grp">'
    + grow(ARCHIVE, "Перенести переписку", "Из файла копии данных: контакты, сообщения, заметки", '')
    + '</div><div class="cap" style="margin-top:8px;">Без файла копии список контактов и чатов начнётся с нуля. Группы не переносятся ни так, ни так — участники пригласят заново.</div>'
    + FILL, "Выбрать файл копии", "Продолжить без переписки", back=False))

# ---------- 10. Пароль файла ----------
fileline = (f'<div class="grp"><div class="gr">{ic(FILE, 22, "#9AA3AD", NF)}<div class="gt"><b>peerage-2026-10-30.backup</b><span>48 МБ · снят 30 октября</span></div>'
            + '<button class="pill" style="background:#26272D;color:#C4C7CC;">Другой</button></div></div>')
bkpass = page("Восстановление: пароль копии", screen(
    "", None, heading("Пароль копии", "Тот, что задавался при создании файла. Он не совпадает с 24 словами."),
    SP24 + fileline + SP16
    + '<div class="inp"><span class="v">••••••••••<span class="cr"></span></span>' + ic(EYE_OFF, 20, "#9AA3AD", NF) + '</div>'
    + '<div class="cap" style="margin-top:8px;">Переписка добавится к той, что уже есть на устройстве; ничего не затирается.</div>' + FILL,
    "Восстановить переписку", "Назад"))

# ---------- 11. Итог ----------
stats = ('<div class="grp">'
         + ''.join(f'<div class="stat">{a}<span>{b}</span></div>' for a, b in
                   [("Контакты", "23"), ("Сообщения", "4 812"), ("Заметки", "37"), ("Журнал звонков", "не было в копии")])
         + '</div>')
done = page("Восстановление: итог", screen(
    "", None, '<div style="height:24px;"></div>' + heading("Переписка перенесена", None, ic(CHECK, 34, "#8BC34A")),
    SP16 + stats + SP16
    + f'<div class="warn">{ic(WARN, 20, "#F2C98A", NF)}<div>Групп в копии нет: история групп видна, но писать в них можно только после нового приглашения.</div></div>'
    + FILL, "Далее — разрешения", back=False))

# ---------- 12. Крупный шрифт ×2.0 ----------
big = page("Приветствие: шрифт ×2.0",
           '<div style="height:24px;flex:none;"></div><div class="ob" style="overflow:hidden;">'
           + '<div class="obt" style="font-size:44px;line-height:52px;">Peerage</div>'
           + '<div class="obs" style="font-size:26px;line-height:34px;color:#8E8E98;">Переписка и звонки без серверов. Личность — ключ на этом устройстве.</div>'
           + SP16 + '<div class="grp">'
           + f'<div class="gr" style="min-height:72px;">{ic(PHONE_OFF, 30, "#9AA3AD", NF)}<div class="gt"><b style="font-size:26px;line-height:32px;">Без номера телефона</b></div></div>'
           + f'<div class="gr" style="min-height:72px;">{ic(BOOK_OFF, 30, "#9AA3AD", NF)}<div class="gt"><b style="font-size:26px;line-height:32px;">Контакты телефона не читаются</b></div></div>'
           + '</div></div>'
           + '<div class="obf" style="box-shadow:0 -16px 24px #08090B;position:relative;">'
           + '<button class="btn pri" style="min-height:64px;font-size:24px;">Создать личность</button>'
           + '<button class="btn ghost" style="min-height:64px;font-size:24px;line-height:30px;">Восстановить по 24 словам</button></div>')

phrase_big = page("24 слова: шрифт ×2.0",
                  top(2) + '<div class="ob" style="overflow:hidden;">'
                  + '<div class="obt" style="font-size:40px;line-height:48px;">24 слова</div>'
                  + '<div class="wg" style="grid-template-columns:1fr;grid-auto-flow:row;grid-template-rows:none;">'
                  + ''.join(f'<div class="w" style="font-size:30px;min-height:54px;"><i style="font-size:22px;width:36px;">{i + 1}</i>{w}</div>' for i, w in enumerate(WORDS[:9]))
                  + '</div></div>'
                  + '<div class="obf" style="box-shadow:0 -16px 24px #08090B;position:relative;"><button class="btn pri" style="min-height:64px;font-size:24px;">Слова записаны</button></div>')

rows = [
    [("Main.dc.html", welcome, "1 · Приветствие: обещания без «мы», два входа"),
     ("Profile.dc.html", profile, "2 · Имя и фото (шаг 1 из 3), можно пропустить"),
     ("KeyIntro.dc.html", keyi, "3 · Копия ключа: зачем и как хранить"),
     ("Phrase.dc.html", phrase, "4 · 24 слова, экран защищён"),
     ("Check.dc.html", check, "5 · ✓ Проверка трёх слов (ввод с подсказкой)"),
     ("Perms.dc.html", perms, "6 · А ✓ Разрешения одним экраном + автозапуск")],
    [("Restore.dc.html", restore, "Восст. 1 · Та же сетка, что при записи; подсказка по буквам"),
     ("Mismatch.dc.html", mismatch, "Восст. 2 · Слова верны, сумма не сходится"),
     ("Restored.dc.html", restored, "Восст. 3 · Личность есть → перенести переписку?"),
     ("BackupPass.dc.html", bkpass, "Восст. 4 · Файл копии + пароль"),
     ("Done.dc.html", done, "Восст. 5 · Итог с цифрами, про группы")],
    [("PermsB.dc.html", perms_b, "6 · Б · Разрешение на экран — отвергнут"),
     ("BigWelcome.dc.html", big, "Шрифт ×2.0: прокрутка, кнопки прибиты"),
     ("BigPhrase.dc.html", phrase_big, "Шрифт ×2.0: слова в один столбец")],
]
row_titles = ["Новая личность", "Восстановление на новом телефоне", "Варианты и крупный шрифт"]
boards, order, notes = {}, [], {}
for r, row in enumerate(rows):
    y = r * 1235
    notes[f"t{r + 1}"] = {"x": 0, "y": y - 300, "text": row_titles[r], "kind": "title1", "maxW": 2860}
    for i, (name, html, title) in enumerate(row):
        with open(os.path.join(P, name), "w", encoding="utf-8") as f:
            f.write(html)
        boards[name] = {"x": i * 492, "y": y, "w": 412, "h": 892, "title": title}
        order.append(name)
canvas = {"v": 3, "createdOnFiles": {"v": 1, "at": "2026-11-01T23:00:00Z"}, "title": "Первый запуск Peerage",
          "launch": {"view": "canvas"}, "pages": [], "boards": boards, "order": order, "notes": notes,
          "designSystems": []}
with open(os.path.join(P, "canvas.json"), "w", encoding="utf-8") as f:
    json.dump(canvas, f, ensure_ascii=False)
print("ok")
