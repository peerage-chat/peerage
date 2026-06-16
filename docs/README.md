# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полное ТЗ — [p2p-messenger-spec.md](p2p-messenger-spec.md).

## Документация

| Документ | Содержание |
|----------|-----------|
| [architecture-review.md](architecture-review.md) | Архитектурный обзор, анализ масштабирования, проблемы и рекомендации |
| [ROADMAP.md](ROADMAP.md) | Дорожная карта: фазы, прогресс, зависимости |
| [SECURITY.md](SECURITY.md) | Модель угроз, крипто-основы, результаты аудита |
| [p2p-messenger-spec.md](p2p-messenger-spec.md) | Исходное техническое задание |

## Архитектура

Общее **ядро на Rust** (`core/`) — единый источник истины для протокола, крипто
и сети. Используется обеими платформами:

- **Desktop** (Windows/Linux/Mac) — Slint (нативный Rust-GUI), зовёт ядро напрямую.
- **Android** — Kotlin + NDK, зовёт ядро через JNI.

Транспорт — оверлейная mesh-сеть **Yggdrasil** (даёт каждому узлу постоянный
IPv6). Yggdrasil поднимается как c-shared библиотека (`libygg.so`) — без TUN, без
root, без wintun. Ядро работает через датаграммы по ed25519-ключу.

### Структура

```
core/                 общее Rust-ядро (кросс-платформенное, чистый Rust)
  src/identity.rs     идентичность: ed25519 → Yggdrasil IPv6, Curve25519, BIP39
  src/crypto.rs       E2E NaCl Box (seal/open) поверх идентичности
  src/protocol.rs     Payload (шифр.) + Envelope (конверт), encode/decode
  src/store.rs        персистентность: Profile (идентичность) + ContactStore (TOFU)
  src/vault.rs        at-rest шифрование (XChaCha20-Poly1305, keyring/keyfile)
  src/settings.rs     пользовательские настройки (аудио/видео кодеков)
  src/media.rs        лёгкое обрамление медиа-датаграмм (демукс чат/медиа, видео-фрагментация)
  examples/whoami.rs  демо: печатает новую идентичность
transport/            Rust: безопасная обёртка над libygg (FFI к yggdrasil-go)
  src/lib.rs          Node: start/listen/add_peer/write/read по ed25519-ключу
  examples/loopback.rs  два узла в процессе, обмен через настоящий Yggdrasil
  examples/netjoin.rs   проба входа в глобальную сеть через публичные пиры
messenger/            Rust: слой сессии (транспорт + протокол + крипто)
  src/lib.rs          Messenger: handshake, presence, keepalive, очередь повторов,
                      room presence beacons
  examples/chat_loopback.rs  E2E текст-чат между двумя узлами (localhost)
  examples/chat.rs    настраиваемый узел чата для реальной сети
  examples/reliability.rs    демо персистентности + presence/Ack/offline/TTL
  examples/media_loopback.rs демо лёгкого медиа-пути (чат+медиа одновременно)
media/                Rust: медиа-цепочка (кодеки, джиттер) поверх транспорта
  src/codec.rs        Opus энкодер/декодер (параметры из AudioSettings)
  src/jitter.rs       джиттер-буфер (reorder по seq, префетч, PLC при потере)
  src/video.rs        H.264 (openh264), PushSource/FrameFeeder для !Send камер
  examples/audio_loopback.rs  синтетика → энкод → сеть → джиттер → декод
group/                Rust: групповое E2E на MLS (RFC 9420, openmls)
  src/lib.rs          Member: create/add/join/encrypt/process (TreeKEM, FS/PCS)
groupchat/            Rust: групповой чат поверх Yggdrasil (MLS+транспорт, serverless)
  src/lib.rs          GroupChat: create/pending/add_member/send_text/handle
  examples/group_chat.rs  3 узла: создание группы, add, обмен — через mesh
call/                 Rust: движок звонка (аудио+видео) поверх media+messenger
  src/lib.rs          Call: демукс по track; аудио (cpal/Opus); видео (H.264);
                      приём→рендер-кадры. Аудио/видео опциональны.
  src/group_call.rs   GroupCall: mesh multicast + audio mix по участникам
  src/capture.rs      ScreenSource (xcap), CameraSource (nokhwa)
  examples/video_loopback.rs  headless: синтетика→H.264→сеть→декод
desktop/              десктоп-приложение (Slint — нативный Rust-GUI, без WebView)
  ui/app.slint        декларативный Holo-интерфейс
  src/main.rs         проводка движка: модели, колбэки, события → UI
androidjni/           Rust: JNI-мост к ядру для Android (cdylib libp2p_android.so)
  src/lib.rs          JNI: старт, pollEvent, чат, контакты, звонки, группы, комнаты
  src/call.rs         CallEngine: push/pull аудио/видео (без cpal — PCM из Kotlin)
  src/groups.rs       GroupManager: актор для GroupChat (!Send) в потоке event_pump
  src/room_call.rs    RoomCallEngine: мультипати аудио комнат (mesh multicast + mix)
android/              Kotlin/Compose (Android app)
  Native.kt           JNI-обвязка: загрузка .so, все external fun
  Model.kt            AppModel: snapshot-state, фоновые потоки (poll/ticker/ops)
  MainActivity.kt     UI: контакты, чат, звонки, группы, оверлей
  CallAudio.kt        AudioRecord/AudioTrack + AEC/NS/AGC
  VideoSender.kt      Camera2 → MediaCodec H.264 (zero-copy) → Rust
  VideoReceiver.kt     Rust → MediaCodec декодер → SurfaceView
  ConnService.kt      Foreground service: WakeLock + WifiLock
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
  build-ygg-android.sh  сборка libygg.so для Android (WSL, NDK)
  build-android-libs.sh  сборка libp2p_android.so (WSL, cargo-ndk)
```

## Решения, отклоняющиеся от ТЗ

- **ed25519 как корневой ключ.** Yggdrasil (v0.4+) использует ed25519, а не
  Curve25519. Поэтому корень идентичности — ed25519, из него выводится адрес
  Yggdrasil, и он же конвертируется в Curve25519 для NaCl Box.
- **RustCrypto вместо libsodium.** Крейт `crypto_box` — та же NaCl Box
  (Curve25519 + XSalsa20 + Poly1305), бинарно совместимая с PyNaCl/libsodium, но
  чистый Rust → проще кросс-компиляция под Android.
- **Yggdrasil без TUN.** Встраиваем ядро `yggdrasil-go` как c-shared библиотеку
  и используем его `PacketConn` напрямую (датаграммы по ключу узла). Не нужны
  права root/админа и драйвер wintun.
- **Медиа без SRTP и без крипто на пакет.** Реалтайм-медиа идёт по тому же
  Yggdrasil-каналу, что уже E2E-зашифрован (ironwood iwe), а источник пакета
  аутентифицирован транспортом. SRTP и пер-пакетный NaCl Box не нужны.
- **Slint вместо Tauri для десктопа.** ТЗ указывает Tauri (WebView), но мы взяли
  Slint — нативный Rust-GUI. Один самодостаточный бинарь без системного WebView.
- **MediaCodec (аппаратный H.264) на Android.** Вместо openh264 (софт-энкод) —
  MediaCodec через Camera2 input-surface (zero-copy). Rust фрагментирует/собирает
  H.264 access-unit'ы, кодирование/декодирование — на железе.

## Прогресс

- [x] Workspace + core-крейт
- [x] Идентичность: keygen ed25519, вывод адреса Yggdrasil, конвертация в
      Curve25519, BIP39-fingerprint (8 слов)
- [x] Вывод адреса сверен бит-в-бит с yggdrasil-go v0.5.13
- [x] FFI-обёртка Yggdrasil: libygg.so ← crate `p2p-ygg`. Узел без TUN/root,
      датаграммы по ed25519-ключу.
- [x] E2E-крипто (NaCl Box) + формат сообщений (Payload/Envelope), юнит-тесты
- [x] Слой сессии `p2p-messenger`: handshake, приём/диспетчер, текст + авто-Ack,
      анти-спуфинг, presence, keepalive, retry до Ack/TTL
- [x] Выход в реальную сеть через публичные пиры
- [x] Персистентность: профиль (идентичность) и контакты (TOFU) на диске,
      at-rest шифрование (vault)
- [x] Надёжность сессии: keepalive-ping, статус онлайн/офлайн, очередь исходящих
      с повтором до Ack и сдачей по TTL
- [x] Desktop UI (Slint, Holo тема): мой код, контакты с онлайн-статусом, чат с ✓✓,
      добавление контакта, звонки (аудио/видео/экран), группы, групповой звонок,
      настройки
- [x] Полировка десктопа: история сообщений, управление контактами, TOFU-входящие,
      поиск, счётчик непрочитанных, время сообщений
- [x] Лёгкий медиа-транспорт: демукс чат/медиа, бинарное обрамление, без
      Ack/крипто на пакет
- [x] Аудио-кодек Opus + джиттер-буфер. Сквозная проверка: 150/150 кадров, RMS 1.00
- [x] Все медиа-параметры настраиваются и сохраняются (`core::settings`)
- [x] **Фаза A — 1:1 аудио-звонок**: сигнализация, движок cpal, mute, UI звонка
- [x] **Фаза B — 1:1 видео**: H.264, фрагментация, камера (nokhwa), экран (xcap),
      рендер в Slint, push-источник для Android MediaProjection
- [x] **Фаза E — группы (MLS, serverless)**: крипто-слой, распространение по mesh,
      admin-sequencer, персистентность, сетевой онбординг
- [x] **Групповые звонки**: GroupCall (full mesh, мультикаст + приём по источникам,
      микс аудио), headless-тест видео от 2 источников
- [x] **Групповой UI в десктопе**: создание/список групп, добавление участников,
      групповой чат с историей, кнопка группового звонка
- [x] **Аудит безопасности (ядро + десктоп)**: H1, H2, M1–M3, L1–L4 закрыты;
      at-rest шифрование, anti-replay, фильтрация источников, скан зависимостей
- [x] **Фаза G — Android (JNI + Compose)**: чат, TOFU, 1:1 аудио/видео звонки,
      групповой чат, foreground service (ConnService), аппаратный H.264 (MediaCodec)
- [x] **H1.0 — Комнаты-звонки (присутствие)**: маячок room presence beacon,
      instant leave signal, TTL 5s, sub-second appear
- [x] **H1.1 — Мультипати аудио комнат**: mesh multicast + audio mix, mesh mesh
      mesh routing по живым участникам
- [ ] Добавление контакта по QR-коду (сейчас — по hex-ключу)
- [ ] H1 остаток: видео в комнатах, собственное превью, переключение фронт/тыл
- [ ] H1.2+: экран настроек медиа (Android), UI-оверхол (иконки вместо эмодзи),
      видео-полировка (аспект SurfaceView)
- [ ] H2 — Контент-адресные блобы (BLAKE3, chunking, resume)
- [ ] H3 — Офлайн-очередь сообщений (serverless)
- [ ] H4 — UI-оболочка (overlay/PiP, тёмный стиль)
- [ ] H5 — Богатые сообщения (картинки/видео/файлы, markdown)
- [ ] H6 — Аудит безопасности (блобы, комнаты, overlay)
- [ ] Торрент-стриминг (Фаза F)

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

Данные (профиль, контакты) хранятся в `app_data_dir`. Для двух копий:

```sh
P2P_DATA_DIR=/tmp/u1 bash scripts/run-desktop.sh   # окно 1
P2P_DATA_DIR=/tmp/u2 bash scripts/run-desktop.sh   # окно 2
```

### Android

```sh
bash scripts/build-ygg-android.sh    # собрать libygg.so (arm64 + x86_64)
bash scripts/build-android-libs.sh  # собрать libp2p_android.so
# затем в Android Studio: gradle assembleDebug
```

### Тулчейн

- **Rust** 1.95+ (`cargo`) — ядро, примеры, тесты.
- **Go** 1.24+ — сборка Yggdrasil-обёртки (WSL/Linux).
- **Android NDK r28c** + cargo-ndk — кросс-компиляция под Android (WSL).
- **Gradle 8.9 + JBR (JDK 21)** — Android-сборка.

## Безопасность

Модель угроз, крипто-основы и результаты аудита — в [`SECURITY.md`](SECURITY.md).
Скан зависимостей: `cargo deny check` (конфиг — `deny.toml`).

## Лицензия

[GPL-3.0-only](LICENSE). Используется GPL-вариант три-лицензии Slint, поэтому
всё приложение копилефтное — исходники остаются открытыми и проверяемыми.
