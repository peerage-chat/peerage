# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полное ТЗ — [p2p-messenger-spec.md](p2p-messenger-spec.md).

## Архитектура

Общее **ядро на Rust** (`core/`) — единый источник истины для протокола, крипто
и сети. Используется обеими платформами:

- **Desktop** (Windows/Linux/Mac) — Tauri, зовёт ядро напрямую.
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
desktop/              Tauri v2 десктоп-приложение (Holo UI)
  src/main.rs         бэкенд: команды + проброс событий движка в UI
  tauri.conf.json     конфиг (статический фронт, без npm)
  ui/                 фронтенд: index.html / style.css / app.js (Holo тема)
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
  run-desktop.sh        сборка и запуск десктоп-приложения (WSL+WSLg)
  setup-desktop-deps.sh установка зависимостей Tauri (WSL, apt)
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
- [x] Desktop UI (Tauri v2, Holo тема): мой код (адрес/ключ/fingerprint),
      контакты с онлайн-статусом, чат с ✓✓, добавление контакта по ключу.
      Бэкенд встраивает движок; запуск в WSL через WSLg (`scripts/run-desktop.sh`)
- [x] Полировка: история сообщений на диске (`core::store::MessageStore`),
      управление контактами (переименование/удаление/инфо), TOFU-добавление
      входящих, поиск, счётчик непрочитанных, время сообщений
- [ ] Добавление контакта по QR-коду (сейчас — по hex-ключу)
- [ ] Android (JNI-мост к ядру)
- [ ] E2E-шифрование сообщений (NaCl Box)
- [ ] Добавление контакта по коду/QR (TOFU)
- [ ] Desktop UI (Tauri)
- [ ] Android (JNI-мост к ядру)
- [ ] Голос / видео / шаринг экрана
- [ ] Группы (MLS) / торрент-стриминг

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

### Десктоп-приложение (Tauri)

Собирается и запускается в WSL; окно отображается на Windows через **WSLg**
(нужен Windows 11 либо Windows 10 с WSLg). Сборка не требует Node/npm —
фронтенд статический.

```sh
bash scripts/setup-desktop-deps.sh   # один раз: WebKitGTK и пр. (apt)
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
