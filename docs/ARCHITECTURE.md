# Архитектура — сквозной разбор системы

Единая «стена текста» по всей системе: что из чего состоит, как данные текут от
нажатия в UI до байтов в меше и обратно. Для глубины каждая тема ссылается на
фокусный документ (инварианты, протокол, токены, ADR). Точка входа в доки —
[docs/README.md](README.md).

> Если правишь систему — сверься с [architecture/invariants.md](architecture/invariants.md)
> (сквозные правила, нарушение которых даёт тихие/катастрофические баги).

---

## 1. Что это и принципы

Serverless P2P-мессенджер с E2E-шифрованием, голосом, видео и шарингом экрана.
Связь — оверлейная mesh-сеть **Yggdrasil** (у каждого узла постоянный IPv6 из
ed25519-ключа). Своего сервера для данных нет; единственное внешнее — публичный
**ntfy.sh**: push-пробуждение спящих устройств и (опц.) relay уже-зашифрованных
групповых сообщений для офлайн-доставки. Сервер видит только байты (имя топика, размер,
время) — не содержимое и не участников.

Принципы:
- **Полный P2P.** Транспорт не доверенный — он лишь маршрутизирует датаграммы по ключу.
- **Единое Rust-ядро.** Один источник истины для протокола/крипто/сети; платформы —
  тонкие адаптеры (Android JNI, десктоп Slint).
- **Фичи = комбинации подсистем-примитивов**, а не отдельные колонны кода (см.
  [SUBSYSTEMS.md](SUBSYSTEMS.md)).
- **Тонкая граница движок↔клиент.** Клиент рендерит снапшот и шлёт команды; домейн-логику
  не считает (см. [ADR-003](adr/ADR-003-engine-api.md), [protocol-spec.md](protocol-spec.md)).

Цель — v1.0 (Android-first) под грант NLnet/NGI Zero. Скоуп — [v1.0-scope.md](v1.0-scope.md),
последовательность — [completion-plan.md](completion-plan.md), [ROADMAP.md](ROADMAP.md).

---

## 2. Карта крейтов (Rust workspace) + Go + Android

```
core/        идентичность, крипто, протокол, БД (SQLCipher), vault, settings, media-обрамление
transport/   безопасная обёртка над libygg (FFI к yggdrasil-go); Node: write/read по ключу
messenger/   слой сессии: handshake, presence, keepalive, надёжная доставка (retry-до-Ack),
             разбор каналов CHAN_CHAT/CHAN_MEDIA, room-presence beacons
media/       медиа-цепочка: Opus (codec), джиттер-буфер, H.264 (video.rs)
group/       MLS-примитив (RFC 9420, openmls): TreeKEM, FS/PCS, encrypt/process
groupchat/   групповой чат поверх Yggdrasil (MLS + транспорт, serverless, admin-sequencer)
call/        движок звонка (десктоп): аудио (cpal/Opus) + видео (H.264), демукс по track
engine/      ДОМЕЙН-ЛОГИКА для клиентов: Engine (фасад), GroupManager (актор !Send),
             RoomCallEngine (мультипати аудио комнат). Граница Command/Event/Snapshot.
ntfy/        клиент ntfy.sh: push-пробуждение + (опц.) relay зашифр. групповых сообщений
             (топик=группа, FIFO-порядок и хранение для офлайн); сервер видит только байты
androidjni/  ТОНКИЙ JNI-мост (cdylib libp2p_android.so): кодек команд/событий поверх engine
desktop/     десктоп-приложение (Slint, нативный Rust-GUI, без WebView); зовёт крейты напрямую
```

```
ygg/         Go: c-shared обёртка вокруг yggdrasil-go (libygg.so) — датаграммный API без TUN/root
android/     Kotlin/Compose Android-клиент (см. §9)
```

Граф зависимостей грубо: `core` ← всё; `transport` → libygg; `messenger` → core+transport;
`media`/`group`/`groupchat`/`call` → messenger; `engine` → core+messenger+media+group+
groupchat+ygg. Клиенты по-разному: **`androidjni` → engine** (тонкий кодек, домейн-логика
в engine), а **`desktop` → messenger+call+groupchat напрямую** (Slint-клиент, не через engine).

---

## 3. Граница движок ↔ клиент (engine)

После M1 домейн-логика живёт в `engine` (крейт `p2p-engine`), а не в JNI/UI.
- **`Engine`** — фасад: владеет сессией мессенджера, контактами, группами, комнатами.
- **Команды** (Command) от клиента → **события** (Event) асинхронно обратно; клиент
  рендерит **Snapshot**. Это формализация ~40 JNI-вызовов как типизированного контракта.
- **`GroupManager`** — актор для `GroupChat`/openmls (они `!Send`): живёт в одном потоке
  `event_pump`, команды приходят каналом. Маршрутизация по `group_id`, персистентность.
- **`RoomCallEngine`** — мультипати аудио комнат (mesh multicast + микс по участникам).

Контракт и миграция — [protocol-spec.md](protocol-spec.md), решение — [ADR-003](adr/ADR-003-engine-api.md).
`androidjni` — тонкий кодек: декодирует JNI-аргументы → команда `engine`, кодирует
событие → JNI. **Никакой логики в JNI/UI.**

Эта граница — **клиентский (Android) путь**. Десктоп (`desktop`) — старше engine и зовёт
крейты (`messenger`/`call`/`groupchat`) напрямую из Slint; постепенная унификация на engine —
направление, а не данность.

---

## 4. Транспорт: Yggdrasil + каналы

- **libygg** (`ygg/libygg/ygg.go`) — c-shared обёртка вокруг `yggdrasil-go`: экспортит
  датаграммный API (`ygg_start/write/read/peers_json/...`). Без TUN, без root, без wintun —
  используем `PacketConn` напрямую (датаграммы по ed25519-ключу узла).
- **`transport`** — безопасная Rust-обёртка (`Node`): start/listen/add_peer/write/read.
- Вход в глобальную сеть — через публичные пиры (`peers/public-peers.txt`).

**Дисциплина каналов** (первый байт датаграммы, `core/src/media.rs`) — ИНВАРИАНТ:
- `CHAN_CHAT = 0x01` — надёжный (id+ACK+retry, `messenger::send_reliable`): чат, ключи
  (MLS Welcome/KeyPackage), управление (RoomInvite/Ack/presence).
- `CHAN_MEDIA = 0x02` — быстрый, без ACK, потеря допустима: только аудио/видео реалтайма.
- **Нельзя** слать ключи/чат по CHAN_MEDIA или гнать медиа через ACK по CHAN_CHAT.

Детали — [invariants.md §2](architecture/invariants.md).

---

## 5. Криптография

- **Идентичность** (`core/src/identity.rs`): корень — **ed25519** (его требует Yggdrasil
  v0.4+). Из него выводится Yggdrasil-IPv6 (сверено бит-в-бит с yggdrasil-go) и
  конвертируется в **Curve25519** для NaCl Box. Fingerprint — 8 слов BIP39 (для
  out-of-band верификации, флаг `verified`).
- **E2E 1:1** (`core/src/crypto.rs`): NaCl **Box** (Curve25519 + XSalsa20 + Poly1305,
  крейт `crypto_box`, бинарно совместим с libsodium). `Payload` (шифр.) + `Envelope`.
- **Группы** (`group`/`groupchat`): **MLS (RFC 9420, openmls)** — TreeKEM, forward secrecy
  и post-compromise security. Serverless: распространение по mesh, **admin-sequencer**
  упорядочивает Commit'ы. ⚠️ MLS чувствителен к порядку и forward-secrecy (см. §6, инвариант §4).
- **At-rest** (`core/src/vault.rs`): XChaCha20-Poly1305, ключ из keyring/keyfile. Шифрует
  БД (SQLCipher-ключ) и файловые хранилища.
- **Медиа без SRTP/пер-пакетного крипто**: реалтайм идёт по уже-E2E Yggdrasil-каналу
  (ironwood iwe), источник аутентифицирован транспортом → доп. крипто не нужно.

Модель угроз и аудит — [SECURITY.md](SECURITY.md), план «за пределы базы» —
[plans/security-hardening.md](plans/security-hardening.md).

---

## 6. Надёжность доставки

- **`send_reliable`** (`messenger/src/lib.rs`) — единый примитив: регистрирует
  `Pending{peer, payload, created, last_sent, notify}`, `maintenance_loop` повторяет каждые
  ~3с до `Ack{id}` или `message_ttl` (300с). Покрывает DM-текст, групповые сообщения,
  MLS-Commit/Welcome. Anti-spoof: Ack принимается только от `peer==src`.
- **Presence/keepalive**: ping-pong, статус онлайн/офлайн, room-presence beacons (TTL 5с,
  instant-leave).
- **Офлайн-очередь** (H3): TTL 7 дней (сейчас захардкожен в Android `Model.kt`, выносится в
  настройки). На стороне core — таблица `outbox` (демон ещё не построен).
- **MLS-инвариант**: любая операция, двигающая состояние (send/receive/commit), ОБЯЗАНА
  `save_group` — иначе после рестарта отправка со «старого» поколения → получатель удалил
  секрет ради FS → сообщения не доходят. Разъехавшиеся группы не лечатся (см. invariants §4).

---

## 7. Персистентность

Два пути (исторически; см. также `architecture-cleanup`):
- **SQLite (core)** — `core/src/db.rs`: SQLCipher (ключ из vault), `journal_mode=WAL`,
  `busy_timeout=5000`, `foreign_keys=ON`. Таблицы: `contacts` (TOFU), `messages`, `outbox`
  (H3). Запись сериализована `Arc<Mutex<Connection>>`. Детали/правила — [invariants.md §3](architecture/invariants.md).
- **Файлы (engine, Android-группы)** — vault-sealed JSON: `.group` (MLS-состояние),
  `.msgs.json` (история), `index.json` (имена). Любое движение MLS → `save_group`.

---

## 8. Медиа-пайплайн

**Аудио:** захват → **Opus** (`media/src/codec.rs`) → CHAN_MEDIA → джиттер-буфер
(`jitter.rs`: reorder по seq, prefetch, PLC при потере) → декод → воспроизведение.
На десктопе захват/вывод — cpal; на Android — `CallAudio.kt` (AudioRecord/AudioTrack +
AEC/NS/AGC), PCM гоняется в Rust через JNI.

**Видео (Android, аппаратный H.264):**
```
Camera2 → MediaCodec-энкодер (input-surface, zero-copy) → H.264 access-unit
  → Rust фрагментирует → CHAN_MEDIA → приёмник собирает → MediaCodec-декодер → Surface
```
- `VideoSender.kt`: Camera2 рендерит прямо в input-surface энкодера + (опц.) в превью-Surface
  одной сессией. Стартовая сторона камеры — параметр `startFront`. Угол сенсора шлётся
  приёмнику для доворота.
- `VideoReceiver.kt`: тянет H.264-пакеты из Rust → декодер → TextureView.
- Размер input-surface ОБЯЗАН быть из `StreamConfigurationMap.getOutputSizes` камеры
  (иначе `onConfigureFailed` — чёрный экран; см. [BUGS.md](BUGS.md), `CameraCaps.kt`).

**Комнаты-звонки (Discord-style):** presence-beacon публикует кто в комнате и что включено
(audio/video флаги); `RoomCallEngine` — full-mesh multicast аудио + микс по источникам;
видео — приём по живым участникам. Приёмная регулировка (подписки на чужие потоки) —
в плане (задача про подписки). Десктоп-аналог: `call/src/group_call.rs`.

Дорастить медиа до устойчивости на реальных сетях (адаптация битрейта, A/V-sync) —
[plans/media-webrtc-parity.md](plans/media-webrtc-parity.md).

---

## 9. Android-клиент (Kotlin/Compose)

**Архитектура потоков (ИНВАРИАНТ):** UI НИКОГДА не зовёт `Native.*` напрямую (любой
JNI-вызов может заблокировать → ANR). Всё через фоновые потоки в `Model.kt`:
- `pollThread` — опрос событий ядра;
- `tickerThread` — периодический статус (пиры/онлайн/presence);
- `ops` — single-thread executor, сериализует операции (отправка, контакты, звонки).

Состояние — **Compose snapshot-state** (`mutableStateOf/List/Map`), безопасно писать из
фоновых потоков (рекомпозиция планируется сама). Сознательно НЕ StateFlow.

**Файлы:**
- Корень: `Native.kt` (JNI-обвязка), `Model.kt` (AppModel: состояние+потоки),
  `MainActivity.kt`, `Theme.kt`/`Tokens.kt` (дизайн-токены), `CallAudio/VideoSender/
  VideoReceiver/CameraPreview/CameraCaps.kt` (медиа), `RoomCallScreen.kt`/`ShareVideoScreen.kt`
  (экраны звонка), `ConnService/CallService/IncomingCallReceiver.kt` (сервисы),
  `VaultKeyManager.kt`.
- `ui/`: `ChatScreen`, `ContactsScreens`, `GroupScreens`, `NetworkScreen`, `NewChatScreen`,
  `SearchScreen`, `SettingsScreen`, `CallUi`, `Components`, `Navigation`, `QrScanner`, `StubScreens`.

**UI-система:** язык/навигация — [design/app-ui.md](design/app-ui.md) (ИСТОЧНИК ИСТИНЫ),
экраны звонка — [design/call-ui.md](design/call-ui.md), цвета/отступы/размеры —
[design/tokens.md](design/tokens.md) + `Tokens.kt`. Эталон — макеты пользователя; новый
код ссылается на `Tokens.*`, не на инлайн-хекс.

---

## 10. Сборка и разработка

**Тулчейн:** Rust 1.95+, Go 1.24+ (libygg, в WSL/Linux), Android NDK r28c + cargo-ndk,
Gradle 8.9 + JBR (JDK 21). Rust/Go-части собираются **через WSL**.

**Android (рабочий цикл):**
```sh
# Rust-only пересборка (быстро; когда менялся только Rust):
bash scripts/rebuild-p2p-android.sh <ABI>      # x86_64 (эмулятор) | arm64-v8a (телефон)
# Полная (когда менялся ygg.go FFI): libygg + Rust
bash scripts/build-android-libs.sh <ABI>
./gradlew :app:assembleDebug                   # APK (Kotlin)
./gradlew ktlintFormat                         # перед коммитом Kotlin
```
`.so` — в `android/app/src/main/jniLibs/<ABI>/` (gitignore). Wireless adb:
`adb pair <ip:port> <code>` → `adb mdns services` → `adb connect <ip:port>`.

**Ядро/десктоп (WSL):** `cargo test -p p2p-core`, `scripts/run-*.sh` (loopback, chat,
realnet, reliability, media, desktop через WSLg).

Подробный crate-map и команды — [README.md](README.md), карта подсистем —
[SUBSYSTEMS.md](SUBSYSTEMS.md).

---

## 11. Где что в документации

| Нужно | Документ |
|---|---|
| Точка входа / навигация | [docs/README.md](README.md) |
| Эта сквозная картина | **ARCHITECTURE.md** (вы здесь) |
| Сквозные правила (FFI/каналы/SQLite/MLS/UI) | [architecture/invariants.md](architecture/invariants.md) |
| Контракт движок↔клиент | [protocol-spec.md](protocol-spec.md), [adr/](adr/) |
| Подсистемы → фичи | [SUBSYSTEMS.md](SUBSYSTEMS.md) |
| Безопасность / модель угроз | [SECURITY.md](SECURITY.md) |
| UI: язык / звонки / токены | [design/app-ui.md](design/app-ui.md), [design/call-ui.md](design/call-ui.md), [design/tokens.md](design/tokens.md) |
| Скоуп v1.0 / порядок работ | [v1.0-scope.md](v1.0-scope.md), [completion-plan.md](completion-plan.md), [ROADMAP.md](ROADMAP.md) |
| Планы (медиа, безопасность) | [plans/](plans/) |
| Известные баги | [BUGS.md](BUGS.md) |
| История/контекст | [archive/](archive/) |
