# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полное ТЗ — [p2p-messenger-spec.md](p2p-messenger-spec.md).

## Архитектура

Общее **ядро на Rust** (`core/`) — единый источник истины для протокола, крипто
и сети. Используется обеими платформами:

- **Desktop** (Windows/Linux/Mac) — Slint (нативный Rust-GUI), зовёт ядро напрямую.
- **Android** — Kotlin + NDK, зовёт ядро через JNI.

Транспорт — оверлейная mesh-сеть **Yggdrasil** (даёт каждому узлу постоянный
IPv6). Yggdrasil поднимает TUN-интерфейс в ОС, а ядро работает поверх через
обычные TCP/UDP сокеты на Yggdrasil-адресе.

### Структура

```
core/                 общее Rust-ядро (кросс-платформенное, чистый Rust)
  src/identity.rs     идентичность: ed25519 → Yggdrasil IPv6, Curve25519, BIP39
  src/crypto.rs       E2E NaCl Box (seal/open) поверх идентичности
  src/protocol.rs     Payload (шифр.) + Envelope (конверт), encode/decode
  src/store.rs        персистентность: Profile (идентичность) + ContactStore (TOFU)
  src/media.rs        лёгкое обрамление медиа-датаграмм (демукс чат/медиа)
  examples/whoami.rs  демо: печатает новую идентичность
transport/            Rust: безопасная обёртка над libygg (FFI к yggdrasil-go)
  src/lib.rs          Node: start/listen/add_peer/write/read по ed25519-ключу
  examples/loopback.rs  два узла в процессе, обмен через настоящий Yggdrasil
  examples/netjoin.rs   проба входа в глобальную сеть через публичные пиры
messenger/            Rust: слой сессии (транспорт + протокол + крипто)
  src/lib.rs          Messenger: handshake, presence, keepalive, очередь повторов
  examples/chat_loopback.rs  E2E текст-чат между двумя узлами (localhost)
  examples/chat.rs    настраиваемый узел чата для реальной сети
  examples/reliability.rs    демо персистентности + presence/Ack/offline/TTL
  examples/media_loopback.rs демо лёгкого медиа-пути (чат+медиа одновременно)
media/                Rust: медиа-цепочка (кодеки, джиттер) поверх транспорта
  src/codec.rs        Opus энкодер/декодер (параметры из AudioSettings)
  src/jitter.rs       джиттер-буфер (reorder по seq, префетч, PLC при потере)
  examples/audio_loopback.rs  синтетика → энкод → сеть → джиттер → декод
group/                Rust: групповое E2E на MLS (RFC 9420, openmls)
  src/lib.rs          Member: create/add/join/encrypt/process (TreeKEM, FS/PCS)
groupchat/            Rust: групповой чат поверх Yggdrasil (MLS+транспорт, serverless)
  src/lib.rs          GroupChat: create/pending/add_member/send_text/handle
  examples/group_chat.rs  3 узла: создание группы, add, обмен — через mesh
ntfy/                 Rust: опциональный клиент ntfy (publish/subscribe) —
                      бэкенд для фонового пробуждения на Android (Фаза G), не обяз.
call/                 Rust: движок звонка (аудио+видео) поверх media+messenger
  src/lib.rs          Call: демукс по track; аудио (cpal/Opus); видео (H.264);
                      приём→рендер-кадры. Аудио/видео опциональны.
  examples/video_loopback.rs  headless: синтетика→H.264→сеть→декод
desktop/              десктоп-приложение (Slint — нативный Rust-GUI, без WebView)
  ui/app.slint        декларативный Holo-интерфейс
  src/main.rs         проводка движка: модели, колбэки, события → UI
ygg/                  Go: c-shared обёртка вокруг yggdrasil-go
  libygg/ygg.go       экспорт датаграммного API (ygg_start/write/read/...)
  cmd/yggaddr/        валидатор адреса средствами самого yggdrasil-go
peers/
  public-peers.txt    список публичных пиров Yggdrasil для входа в сеть
scripts/
  build-ygg.sh          сборка libygg.so (WSL)
  run-loopback.sh       транспортный loopback-тест (WSL)
  run-chat.sh           E2E чат-тест по localhost (WSL)
  run-netjoin.sh        проба входа в глобальную сеть (WSL)
  run-realnet-chat.sh   E2E чат двух процессов через реальную сеть (WSL)
  run-reliability.sh    демо персистентности и надёжности сессии (WSL)
  run-media.sh          демо лёгкого медиа-пути (WSL)
  run-audio.sh          тесты кодека/джиттера + аудио-loopback (WSL)
  run-desktop.sh        сборка и запуск десктоп-приложения (WSL+WSLg)
  setup-desktop-deps.sh установка зависимостей Slint (WSL, apt)
  verify-persist.sh     две копии через реальную сеть + проверка истории на диске
  validate-ygg-addr.sh  кросс-проверка Go vs Rust вывода адреса
```

## Решения, отклоняющиеся от ТЗ

- **ed25519 как корневой ключ.** Yggdrasil (v0.4+) использует ed25519, а не
  Curve25519. Поэтому корень идентичности — ed25519, из него выводится адрес
  Yggdrasil, и он же конвертируется в Curve25519 для NaCl Box (как
  `crypto_sign_ed25519_pk_to_curve25519`).
- **RustCrypto вместо libsodium.** Крейт `crypto_box` — это та же NaCl Box
  (Curve25519 + XSalsa20 + Poly1305), бинарно совместимая с PyNaCl/libsodium, но
  чистый Rust → проще кросс-компиляция под Android, не нужен C-тулчейн.
- **Yggdrasil без TUN.** Встраиваем ядро `yggdrasil-go` как c-shared библиотеку
  и используем его `PacketConn` напрямую (датаграммы по ключу узла) вместо
  поднятия TUN-интерфейса. Не нужны права root/админа и драйвер wintun — это
  настоящее библиотечное встраивание.
- **Медиа без SRTP и без крипто на пакет.** Реалтайм-медиа идёт по тому же
  Yggdrasil-каналу, что уже E2E-зашифрован (ironwood iwe), а источник пакета
  аутентифицирован транспортом. Поэтому SRTP и пер-пакетный NaCl Box не нужны —
  это экономит CPU и байты (приоритет проекта — минимум ресурсов).
- **Slint вместо Tauri для десктопа.** ТЗ указывает Tauri (WebView), но мы взяли
  Slint — нативный Rust-GUI. Один самодостаточный бинарь без системного WebView,
  UI вызывает движок напрямую (без слоя команд/сериализации). Holo-тема рисуется
  декларативно. Ядро/транспорт/мессенджер UI-агностичны и не затронуты.

## Важные технические находки

- **Сессия ironwood двунаправленная.** Зашифрованная сессия Yggdrasil (ironwood
  `iwe`) устанавливается только при обмене пакетами В ОБЕ СТОРОНЫ. Если один узел
  только шлёт, а второй только принимает — рукопожатие не завершается и пакеты
  теряются (даже при ретраях). Поэтому при установлении связи оба пира должны
  слать: на уровне протокола сообщений нужен handshake/ping с обеих сторон.

## Прогресс

- [x] Workspace + core-крейт
- [x] Идентичность: keygen ed25519, вывод адреса Yggdrasil, конвертация в
      Curve25519, BIP39-fingerprint (8 слов)
- [x] Вывод адреса сверен бит-в-бит с yggdrasil-go v0.5.13 (регрессионный тест +
      `scripts/validate-ygg-addr.sh`)
- [x] FFI-обёртка Yggdrasil: libygg.so (c-shared) ← crate `p2p-ygg`. Узел без
      TUN/root, датаграммы по ed25519-ключу. Loopback-тест: два узла в процессе
      обмениваются через настоящий Yggdrasil-mesh (`scripts/run-loopback.sh`)
- [x] E2E-крипто (NaCl Box) + формат сообщений (Payload/Envelope) в ядре,
      покрыто юнит-тестами (roundtrip, симметрия, чужой получатель, подделка)
- [x] Слой сессии `p2p-messenger`: handshake, приём/диспетчер, текст + авто-Ack,
      анти-спуфинг. Сквозной E2E текст-чат между узлами (`scripts/run-chat.sh`)
- [x] Выход в реальную сеть: пиринг с публичной инфраструктурой Yggdrasil
      (`peers/public-peers.txt`). Два независимых процесса, подключённых ТОЛЬКО
      к публичным пирам, обмениваются E2E-сообщениями через глобальный mesh
      (`scripts/run-realnet-chat.sh`) — путь, идентичный связи двух устройств
- [x] Персистентность: профиль (идентичность) и контакты (TOFU) на диске
      (`core::store`, юнит-тесты)
- [x] Надёжность сессии: keepalive-ping, статус онлайн/офлайн (presence),
      очередь исходящих с повтором до Ack и сдачей по TTL, чистая остановка
      (`scripts/run-reliability.sh`)
- [x] Desktop UI (Slint, нативный Rust-GUI, Holo тема): мой код (адрес/ключ/
      fingerprint), контакты с онлайн-статусом, чат с ✓✓, добавление контакта.
      UI вызывает движок напрямую; запуск в WSL через WSLg (`scripts/run-desktop.sh`).
      Без WebView/Node (переход с Tauri — отклонение от ТЗ ради единого бинаря)
- [x] Полировка: история сообщений на диске (`core::store::MessageStore`),
      управление контактами (переименование/удаление/инфо), TOFU-добавление
      входящих, поиск, счётчик непрочитанных, время сообщений
- [x] Лёгкий медиа-транспорт: демукс чат/медиа по байту канала, бинарное
      обрамление (track/seq/ts), отправка без аллокаций, приём через bounded-канал
      с drop-on-full, без Ack/крипто на пакет (E2E даёт транспорт). Проверено:
      чат и поток медиа идут одновременно по одному каналу (`scripts/run-media.sh`)
- [x] Аудио-кодек Opus + джиттер-буфер. Сквозная проверка на синтетике:
      150/150 кадров, RMS 1.00 (`scripts/run-audio.sh`)
- [x] Все медиа-параметры настраиваются пользователем и сохраняются
      (`core::settings`, экран «Настройки» в UI)
- [x] **Фаза A — 1:1 аудио-звонок**: сигнализация (invite/accept/end, проверено),
      движок `p2p-call` (cpal: захват→Opus→сеть→джиттер→Opus→вывод, mute), UI
      звонка в десктопе. Реальный звук — ручной тест на железе
- [~] **Фаза B — видео**: фрагментация кадров ✓, H.264 кодек ✓, унифицированный
      `Call` (аудио+видео демукс) ✓. Сквозная проверка: синтетика→H.264→сеть→
      декод, 10 кадров (`scripts/run-video.sh`). Осталось: захват камеры/экрана
      и рендер в UI
- [x] Захват экрана `ScreenSource` (xcap, за фичей `capture`) + push-источник
      `PushSource`/`FrameFeeder` (для Android MediaProjection через JNI — шаринг
      экрана на телефоне тем же конвейером). Сборка проверена
- [x] Видео в UI звонка: рендер входящих кадров (Slint Image), кнопки 📹 камера
      и 🖥 экран. Камера (nokhwa, `!Send`) идёт через push-источник в отдельном
      потоке. **Фаза B завершена** (1:1 видео: камера/экран). Реальный захват — на железе
- [ ] Добавление контакта по QR-коду (сейчас — по hex-ключу)
- [x] **Фаза E (чат) — группы (MLS), serverless**: крипто-слой `p2p-group`
      (openmls, TreeKEM, FS/PCS) + `p2p-groupchat` — раздача MLS-сообщений по
      Yggdrasil, порядок Commit через админ-секвенсора, БЕЗ сервера. Сквозной
      тест на 3 узлах через mesh (`scripts/run-group.sh`) ✓
- [x] Группы: персистентность состояния на диск (save/load) ✓; групповые звонки
      `GroupCall` (full mesh, мультикаст + приём по источникам, микс аудио) —
      headless-тест видео от 2 источников через mesh (`scripts/run-group-call.sh`) ✓
- [x] **Групповой UI в десктопе**: создание группы, список, добавление участников
      (сетевой онбординг), групповой чат, кнопка группового аудио-звонка,
      персистентность групп. Десктоп-клиент функционально полный
- [ ] Мелочи: история групповых сообщений на диск (сейчас только MLS-состояние)
- [ ] Android (JNI-мост к ядру) — в т.ч. телефонный шаринг экрана и пробуждение
      в фоне (serverless foreground service / опц. self-hosted ntfy — выбор юзера)
- [ ] Торрент-стриминг

## Разработка

```sh
cargo test -p p2p-core                   # тесты ядра (Windows/любая платформа)
cargo run -p p2p-core --example whoami   # сгенерировать идентичность
bash scripts/validate-ygg-addr.sh        # (WSL) сверить адрес с yggdrasil-go
bash scripts/run-chat.sh                 # (WSL) E2E чат по localhost
bash scripts/run-netjoin.sh              # (WSL) проверить вход в глобальную сеть
bash scripts/run-realnet-chat.sh         # (WSL) E2E чат двух узлов через интернет
bash scripts/run-reliability.sh          # (WSL) персистентность + presence/offline/TTL
```

Публичные пиры в `peers/public-peers.txt` со временем устаревают — актуальный
список: <https://publicpeers.neilalexander.dev/>.

### Десктоп-приложение (Slint)

Нативный Rust-GUI (Slint) — один бинарь, **без WebView/Node**, UI вызывает движок
напрямую в процессе. Собирается и запускается в WSL; окно отображается на Windows
через **WSLg**.

```sh
bash scripts/setup-desktop-deps.sh   # один раз: рантайм-библиотеки Slint (apt)
bash scripts/run-desktop.sh          # собрать и запустить приложение
```

Данные (профиль, контакты) хранятся в `app_data_dir`. Чтобы запустить **две
копии локально** для проверки чата, задайте разные каталоги:

```sh
P2P_DATA_DIR=/tmp/u1 bash scripts/run-desktop.sh   # окно 1
P2P_DATA_DIR=/tmp/u2 bash scripts/run-desktop.sh   # окно 2
```

В каждом окне «Мой код» даёт публичный ключ; добавьте его в другом окне через
«+», и можно переписываться через глобальную сеть Yggdrasil.

### Тулчейн

- **Rust** 1.95+ (`cargo`) — ядро, примеры, тесты.
- **Go** 1.24+ — сборка Yggdrasil-обёртки. Ставится в WSL/Linux
  (`yggdrasil-go` + cgo собираются на Linux без возни с mingw/wintun).
  Сейчас Yggdrasil-часть разрабатывается под Linux-таргет; Windows-упаковка —
  отдельная задача на этапе релиза.

## TODO / валидация

- Вывод адреса Yggdrasil (`addr_for_key`) портирован из yggdrasil-go и покрыт
  тестами на детерминизм/префикс. Нужно кросс-проверить с реальным узлом
  Yggdrasil на этапе интеграции — сверить, что адрес совпадает с тем, что
  сообщает сам `yggdrasil` для того же ключа.
