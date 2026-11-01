# Макеты перепроектирования UI (с 2026-11-01)

Исходники холстов прохода [ui-redesign.md](../../ui-redesign.md). Каждая папка — один холст:
`canvas.json` (раскладка артбордов и их подписи) + по файлу `*.dc.html` на экран. Живая
версия — артефакт по ссылке ниже; здесь — копия, чтобы макеты не зависели от артефактов.

Отличие от соседних `../*.html`: те — макеты июня 2026 на `tokens.css`; эти — новый язык
(«лист», Golos Text, радиусы 8 / 18 / 28). Где они расходятся, верно то, что здесь
и в `ui-redesign.md`.

`*.dc.html` — формат холста: внутри обычная HTML-раскладка с инлайн-стилями, обёрнутая
в `<x-dc>`; `support.js` подставляет холст и локально его нет. В браузере файл всё равно
открывается и показывает экран (412 × 892), без панелей холста.

| Папка | Холст | Артефакт |
|---|---|---|
| `now/` | Как сейчас — снимки текущего UI с эмулятора (`peerage-ui-now.html` + `shots/`) | [J5Qc8Q5XLoJNgQjn1pchMc](https://claude.ai/artifact/J5Qc8Q5XLoJNgQjn1pchMc) |
| `nav/` | Навигация | [FcwN7jG989ya5jWhTrZtmw](https://claude.ai/artifact/FcwN7jG989ya5jWhTrZtmw) |
| `home/` | Главная | [StLbzbtN8DHV94J5EN2nMs](https://claude.ai/artifact/StLbzbtN8DHV94J5EN2nMs) |
| `chat/` | Чат | [8kECtWmQbMyhHC8vyEqAz7](https://claude.ai/artifact/8kECtWmQbMyhHC8vyEqAz7) |
| `prof/` | Профили контакта и группы | [SUe55PFAWuvwvTz7yKeTuY](https://claude.ai/artifact/SUe55PFAWuvwvTz7yKeTuY) |
| `type/` | Типографика и цвет | [Krah45uyjTtc88bxveN9Uc](https://claude.ai/artifact/Krah45uyjTtc88bxveN9Uc) |
| `empty/` | Пустые состояния, напоминания о копиях | [7YmisSQR4tPRnaVwFkVgxA](https://claude.ai/artifact/7YmisSQR4tPRnaVwFkVgxA) |
| `screens/` | Загрузка, контакты, звонки, сеть, хранилище, чат группы, «Медиа и файлы» | [MSNNAenF118SNhejtg4WKB](https://claude.ai/artifact/MSNNAenF118SNhejtg4WKB) |
| `settings/` | Категории настроек | [29zmh8gqdDxJxuPEJbiPnV](https://claude.ai/artifact/29zmh8gqdDxJxuPEJbiPnV) |
| `notif/` | Уведомления | [TYfNXnzhux2Jtrn8cnDURV](https://claude.ai/artifact/TYfNXnzhux2Jtrn8cnDURV) |
| `chatsearch/` | Поиск в чате | [Dk9N8gDEytw4hy4FHQuBFs](https://claude.ai/artifact/Dk9N8gDEytw4hy4FHQuBFs) |
| `chatact/` | Действия в чате, поле ввода, реакции | [E39iZbaK59v7yLhxPMrtmR](https://claude.ai/artifact/E39iZbaK59v7yLhxPMrtmR) |
| `special/` | Особые сообщения в ленте | [TPP2vbMCLLRmAudpB7s9Fc](https://claude.ai/artifact/TPP2vbMCLLRmAudpB7s9Fc) |
| `requests/` | Запросы и приглашения | [7BjgsS3G7Nus8zQPH6cCLS](https://claude.ai/artifact/7BjgsS3G7Nus8zQPH6cCLS) |
| `viewer/` | Просмотр фото и видео | [SYFqEhupR4L2xrHjgFJ2Cq](https://claude.ai/artifact/SYFqEhupR4L2xrHjgFJ2Cq) |
| `onboard/` | Первый запуск: новая личность, 24 слова, восстановление с переносом переписки, разрешения | [FEwZX9VE2FDL4SbmxWG4JN](https://claude.ai/artifact/FEwZX9VE2FDL4SbmxWG4JN) |
| `security/` | Безопасность: сверка ключа контакта, блокировка, пароль хранилища, показ ключа | [NMcfp1QdEHdvMuvKzpRicg](https://claude.ai/artifact/NMcfp1QdEHdvMuvKzpRicg) |
| `people/` | Контакты и группы: добавить контакт, свой код и профиль, создать группу, пригласить, права админа | [5fL9zC3CyvGEgQ1Lopxmxg](https://claude.ai/artifact/5fL9zC3CyvGEgQ1Lopxmxg) |
| `misc/` | Второстепенные экраны: сохранённое, исходящие, поиск, папки, заблокированные, поток трафика, смена адреса, копия данных | [3Lm7Fu8XokunU7M2vQiFsp](https://claude.ai/artifact/3Lm7Fu8XokunU7M2vQiFsp) |

`_gen/` — генераторы последних холстов (`python -I _gen/gen_chatact.py <папка>` пишет
`<папка>/project/…`); остальные холсты писались вручную. Холст правится в артефакте или
здесь — после правки в одном месте перенести её в другое, иначе копии разойдутся.
