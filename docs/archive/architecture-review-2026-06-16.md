> ⚠️ **АРХИВ — исторический снапшот (2026-06-16).** Анализ архитектуры перед
> масштабированием; многое с тех пор изменилось. Актуальная картина системы —
> [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md). Оставлено для контекста «как мыслили».

# Архитектурный обзор и анализ масштабируемости

> Дата: 2026-06-16
> Контекст: анализ текущей архитектуры перед масштабированием до full-featured
> мессенджера (Telegram/Discord-level) в serverless P2P условиях.

---

## Содержание

1. [Текущая архитектура (сводка)](#1-текущая-архитектура-сводка)
2. [Граф зависимостей крейтов](#2-граф-зависимостей-крейтов)
3. [Проблемы масштабирования](#3-проблемы-масштабирования)
4. [Матрица влияния на фазы H1–H6](#4-матрица-влияния-на-фазы-h1h6)
5. [Рекомендуемый порядок исправлений](#5-рекомендуемый-порядок-исправлений)

---

## 1. Текущая архитектура (сводка)

### Workspace (10 Rust-крейтов + 1 Go + Kotlin/Compose)

| Крейт | Назначение | LOC | Платформа |
|-------|-----------|-----|-----------|
| `p2p-core` | Идентичность (ed25519), крипто (NaCl Box), протокол, хранилище (JSON), настройки, vault (at-rest), медиа-фрейминг | ~800 | Общ. |
| `p2p-ygg` | FFI-обёртка над `yggdrasil-go` (c-shared). Датаграммы по ключу, без TUN/root | ~350 | Linux/Android |
| `p2p-messenger` | Надёжный сессионный слой: handshake, presence, keepalive, Ack/retry/TTL, анти-replay, call signalling, room beacons | ~540 | Общ. |
| `p2p-media` | Opus аудио + джиттер, H.264 видео (openh264), PushSource для `!Send` камер | ~500 | Общ. |
| `p2p-call` | 1:1 звонок (cpal + video), GroupCall (mesh multicast + audio mix) | ~600 | Desktop |
| `p2p-group` | MLS (RFC 9420, openmls): TreeKEM, FS/PCS | ~400 | Общ. |
| `p2p-groupchat` | Распространение MLS по Yggdrasil, admin-sequencer, serverless | ~300 | Общ. |
| `p2p-ntfy` | Опц. клиент ntfy.sh (publish/subscribe) | ~60 | Общ. |
| `p2p-desktop` | GUI на Slint (Holo-тема), ~1080 строк | ~1080 | Desktop |
| `p2p-androidjni` | `libp2p_android.so` (cdylib) — JNI-мост, polling-модель | ~1060 | Android |

### Транспортная модель

```
Yggdrasil mesh (overlay IPv6)
  ↓ ed25519-keyed datagrams (ironwood iwe — E2E ratchet)
Transport (p2p-ygg → libygg.so via FFI)
  ↓ CHAN_CHAT / CHAN_MEDIA demux
Messenger (handshake, presence, reliable delivery)
  ↓ Event channel (mpsc) → Application
```

### Криптографический стек

| Слой | Алгоритм | Назначение |
|------|----------|-----------|
| Идентичность | ed25519 | Корень → Yggdrasil IPv6, Curve25519, BIP39 fingerprint |
| 1:1 E2E | NaCl Box (XSalsa20-Poly1305) | Шифрование сообщений |
| Транспорт | ironwood `iwe` (ratchet) | FS, PCS, anti-replay на mesh |
| Группы | MLS (RFC 9420) | TreeKEM, X25519/AES128-GCM/SHA256/Ed25519 |
| At-rest | XChaCha20-Poly1305 | Ключ в keyring/keyfile, данные зашифрованы |

---

## 2. Граф зависимостей крейтов

```
                        ┌─ desktop (Slint UI)
                        │
         ┌──────────────┼──────────────┐
         │              │              │
        core         messenger       ntfy
         │              │         (опц. push)
         │              │
         │          ┌───┼───┐
         │          │   │   │
         │        media call groupchat
         │          │   │     │
         │          │   │   group
         │          │   │
         │          │   └── androidjni (cdylib → JNI)
         │          │
        transport (FFI → libygg.so, Go)
```

**Ключевой инвариант:** `core` не зависит ни от чего (чистый Rust). `transport`
зависит только от внешней Go-библиотеки. Все остальные крейты зависят от `core`.
Дефолтная сборка (`cargo build`) включает только `core` (transport — Linux-only).

---

## 3. Проблемы масштабирования

### P1. JSON-протокол — тупик для производительности и расширяемости

**Файл:** `core/src/protocol.rs:83-119`

**Суть:** Конверт сериализуется как JSON, payload — JSON, зашифрованные данные —
hex-кодируются внутри JSON. Цепочка: `Payload → serde_json::to_vec → crypto::seal →
hex::encode → Envelope → serde_json::to_vec`.

**Проблемы:**
- **3× overhead по размеру** на каждое сообщение (JSON keys + hex expansion 2×).
- **Аллокации на каждой кодировке/декодировке** (JSON → Vec<u8> → String → Vec<u8>).
- Нет бинарного пути для signalling (CallInvite, RoomPresence) — маленькие control-
  сообщения несут JSON-overhead в 200+ байт.
- **Нет расширяемости полей:** добавление нового поля в Payload = breaking change для
  всех клиентов. `serde(default)` есть только на `verified` — остальные поля
  обязательны.

**Влияние:** H5 (богатые сообщения), H2 (блобы-метаданные), H1 (frequent presence).

**Решение:** Перейти на бинарный формат (postcard/bincode/кастомный TLV). Добавить
ignorable extension fields. Разделить конверт (fixed binary header) от payload.

---

### P2. `Payload` — монолитный enum без versioning

**Файл:** `core/src/protocol.rs:24-52`

**Суть:** Все типы сообщений в одном enum. Новые типы (reactions, edit, delete,
typing, read-receipts, блобы, голосования) = breaking change протокола.

**Проблемы:**
- Нет per-command versioning (только `v: 1` на весь конверт).
- Нет `#[serde(default)]` на новых вариантах — старый клиент паникует при
  неизвестном variant.
- Нет typed attachments / extension blocks.

**Влияние:** Любая новая фича (H1–H6) требует синхронного обновления всех клиентов.

**Решение:** Ввести extension blocks / typed content. При декодировании неизвестные
типы помечать как `Unknown { type_id, data }` и игнорировать. Версионировать
payload-format отдельно от envelope-version.

---

### P3. Messenger — единая очередь событий без приоритизации

**Файл:** `messenger/src/lib.rs:380-483`

**Суть:** Один `receive_loop` читает все датаграммы, один `mpsc::channel<Event>` —
единая очередь на чат, звонки, группы, presence, комнаты.

**Проблемы:**
- Медиа идёт через отдельный bounded `media_tx` (drop-on-full) — это правильно. Но
  **все сигнальные сообщения** конкурируют за один `Receiver<Event>`.
- Burst presence-событий (10 участников комнаты × 2-сек маячок = 5 events/sec)
  засоряет очередь и задерживает CallInvite или Text.
- Нет обратного давления: `let _ = self.tx.send(ev)` — события молча теряются при
  переполнении.
- Нет подписок по интересам (consumer получает ВСЕ события, фильтрует сам).

**Влияние:** H1 (постоянные комнаты, частые presence), H3 (офлайн-очередь).

**Решение:** Multi-queue с приоритетами (calls > text > presence). Или
типизированные подписки (consumer регистрирует интерес в определённых типах).

> ⚡ **Частично решено на уровне транспорта** (`e4d4f80`): `ygg.go` теперь маршрутизирует
> write-запросы в две очереди — `reliableCh` (аудио + чат, blocking) и `videoCh`
> (video/screen, try-send drop). `writerLoop` всегда дренирует reliable-очередь перед
> видео. Это решает приоритизацию на сетевом уровне. **App-level Event-очередь**
> (messenger/src/lib.rs) по-прежнему единая без приоритетов.

---

### P4. Хранилище — полная перезапись файла на каждую операцию

**Файл:** `core/src/store.rs:296-299, 331-349`

**Суть:** Каждый `append()`, `set_status()`, `add()` → `save()` → сериализует ВСЮ
структуру → `vault::seal()` → `fs::write()` всего файла.

```rust
pub fn append(&mut self, peer, msg) {
    self.log.entry(*peer).or_default().push(msg);
    self.save();  // ← full rewrite
}
```

**Проблемы:**
- 100 контактов × 1000 сообщений → файл ~1 МБ. Каждое входящее сообщение =
  сериализация + шифрование + запись 1 МБ.
- На Android с flash-памятью — ускоренный износ и блокировка мьютекса на 10-50 мс.
- Нет append-only лога, нет periodic compaction.
- `MessageStore`, `ContactStore`, `GroupManager` — каждая ведёт свой файл с тем же
  паттерном.

**Влияние:** H3 (офлайн-очередь — ещё больше записей), H5 (богатые сообщения —
метаданные), любой рост истории.

**Решение:** Перейти на SQLite (rusqlite — чистый Rust, без внешних зависимостей).
Или append-only WAL с periodic compaction. Per-chat шарды как минимум.

---

### P5. Глобальное состояние через `static` в androidjni — тупик для N-сессий

**Файл:** `androidjni/src/lib.rs:57-91`

**Суть:**
```rust
static CALL: Mutex<Option<Arc<CallEngine>>> = Mutex::new(None);
static APP: OnceLock<Arc<App>> = OnceLock::new();
static ROOM_CALL: Mutex<Option<RoomCall>> = Mutex::new(None);
```

**Проблемы:**
- Один звонок на процесс. Один RoomCall на процесс. Несколько комнат одновременно —
  невозможно.
- Нет абстракции «сессия» — нельзя создать второй Messenger для тестирования.
- Глобальные мьютексы ⇒ deadlock-риск при пересекающихся локах (`CALL` +
  `rooms()` + `APP`).
- `current_call()` берёт мьютекс и делает `clone()` Arc — безопасно, но хрупко при
  изменениях.

**Влияние:** H1 (несколько комнат одновременно), H4 (overlay + звонок).

**Решение:** Ввести `Session`-объект с идентификатором. JNI принимает session_id.
RoomCall → `HashMap<gid, RoomCall>`. Тестирование через отдельные сессии.

> ✅ **Частично решено** (`235ee6b`): `Session`-объект введён, `APP`/`CALL`/`ROOM_CALL`
> статики мигрированы в session-based подход. RoomCall per-group пока не реализован
> (один ROOM_CALL на процесс), но deadlock-риск снижен и основа для H4 есть.

---

### P6. Polling-модель событий — задержка и CPU waste

**Файл:** `androidjni/src/lib.rs:451-460`

**Суть:** Kotlin вызывает `pollEvent()` → получает одну JSON-строку → парсит.

**Проблемы:**
- Задержка = poll-interval (50-100 мс). Для typing-индикаторов и presence —
  заметное «дёргание».
- CPU/battery waste при частом опросе.
- JSON-парсинг в Rust (десятки `serde_json::json!` макросов) + JSON-парсинг в Kotlin
  на каждый event.
- Нет push-коллбэков в приложение.

**Влияние:** H1 (typing, presence), H4 (overlay-реакции), UX в целом.

**Решение:** Перейти на callback-модель (Kotlin 注册 JNI callback, Rust зовёт его
из event_pump). Или typed binary events вместо JSON.

---

### P7. GroupChat !Send — сериализация всей групповой работы

**Файл:** `androidjni/src/groups.rs` (референс), `androidjni/src/lib.rs:170-207`

**Суть:** `GroupChat` не `Send`, поэтому живёт в одном потоке `event_pump`. Все
групповые команды идут через `GroupCmd` → `Sender` → `Receiver`.

**Проблемы:**
- Если групповой обработчик тормозит (MLS crypto: TreeKEM, encrypt/decrypt — мс на
  операцию), **все остальные события** (чат, звонки) блокируются в event_pump.
- Нет параллелизма между группами.
- Блокирующие `group_query()` — JNI-вызов ждёт ответа через `mpsc::recv()`. Если
  event_pump занят — JNI-поток висит.

**Влияние:** H1 (группы + звонки одновременно), активные группы (10+).

**Решение:** Вынести MLS-crypto в `Send`-обёртку (как в `CallEngine` — push/pull
через каналы). Или async runtime (tokio) с `spawn_blocking`.

---

### P8. Нет модели «устройств» — multi-device невозможен

**Суть:** Один identity = один ed25519 ключ = одно «устройство». Нет разделения
session keys от long-term identity.

**Проблемы:**
- Телефон + десктоп одновременно — невозможны (конфликт ключей Yggdrasil).
- Нет sender keys (как в Signal: отдельный ключ для группы, общий для устройств
  одного пользователя).
- Компрометация устройства = компрометация всей истории (H2 — FS обеспечивается
  только ratchet на transport-уровне, без Double Ratchet на app-level).

**Влияние:** Базовая feature любого мессенджера. Блокирует паритет с Telegram.

**Решение:** Ввести device keys (long-term identity + per-device session key).
Yggdrasil-подключение — по device key. Identity binding — криптографическое.
Это крупная рефакторинг-задача, затрагивающая core + transport + messenger.

---

### P9. Full-mesh для групповых звонков — не масштабируется

**Файл:** `call/src/group_call.rs:194`, `androidjni/src/room_call.rs`

**Суть:** Каждый участник шлёт аудио/видео каждому (N×(N-1) потоков).

**Проблемы:**
- 10 участников = 9 видеопотоков × 10 отправителей = 90 потоков.
- Yggdrasil overlay — переменная latenсия/throughput. Full-mesh видео при 10+
  участниках = катастрофа по полосе.
- Даже аудио: 10 участников = 9 × 96 kbps = 864 kbps исходящего.

**Влияние:** H1 (постоянные комнаты 5-20+ участников). Критический блокер.

**Решение:**
- SFU (Selective Forwarding Unit) — но это «сервер», противоречит serverless.
- Audio-mesh + selective video (только говорящий, voice-activation).
- Simulcast / layered coding с адаптивным выбором.
- Или hybrid: один участник назначается «relay» (выбирается динамически).

---

### P10. Нет модели потоков (threading model)

**Суть:** Сырые `thread::spawn` с manual `AtomicBool` для остановки. Нет tokio/async.

**Проблемы:**
- Количество сырых потоков растёт хаотично (receive_loop, maintenance_loop,
  event_pump, call encode/decode, room call, connect threads, bench threads...).
- TODO в коде: `// TODO: грациозное завершение фоновых потоков`.
- Нет structured concurrency — потоки живут «пока процесс».
- Race conditions при shutdown (shutdown() → running=false, но потоки могут быть в
  блокирующем `node.read()`).

**Влияние:** Каждая новая фича = 2-3 новых потока. Отладка конкуренции — кошмар.

**Решение:** Ввести tokio runtime (или хотя бы threadpool с задачами). Graceful
shutdown через cancellation token. Event-driven модель вместо blocking reads.

---

### P11. Нет транспортного слоя для больших бинарных данных

**Суть:** Yggdrasil — датаграммы (MTU ~1300 байт). Нет chunking/resume/dedup
для больших blobs.

**Влияние:** H2 (контент-адресные блобы), H5 (картинки/видео/файлы в чате).

**Решение:** Отдельный crate `p2p-blob` с BLAKE3-адресацией, chunking (4-64 КБ),
resumable transfers, pull-модель. Запланирован в ROADMAP (H2) — нужно реализовать
до H5.

---

### P12. Крипто-сложность: множество не связанных механизмов

**Суть:**
- ed25519 → Curve25519 (ручная конверсия)
- x25519-secret через SHA512(seed)[:32]
- NaCl Box (app-level E2E)
- ironwood iwe (transport ratchet)
- MLS (TreeKEM, отдельный ciphersuite)
- XChaCha20-Poly1305 (at-rest vault)
- (будущее) BLAKE3 для blob-адресации

Нет единой key schedule или крипто-политики. Каждый механизм — отдельно.

**Влияние:** Каждое добавление (device keys, sender keys, ephemeral) увеличивает
поверхность для крипто-ошибок.

**Решение:** Документировать key hierarchy. При добавлении device keys —
централизовать key derivation.

---

### P13. Нет офлайн-доставки (serverless gap)

**Суть:** Сообщение отправляется повторно до Ack или TTL (5 мин). Если оба пира
офлайн — сообщение теряется. Нет очереди на диске отправителя, которая переживает
перезапуск приложения.

**Влияние:** H3 (офлайн-очередь). Фундамент для «как Telegram».

**Решение:** Запланировано в ROADMAP (H3): очередь исходящих на диске + фоновый
flush по событию «пир в сети». Требует P4 (хранилище) и P10 (threading).

---

## 4. Матрица влияния на фазы H1–H6

| Проблема | H1 (комнаты) | H2 (блобы) | H3 (офлайн) | H4 (UI overlay) | H5 (богатые) | H6 (аудит) |
|----------|:---:|:---:|:---:|:---:|:---:|:---:|
| **P1** JSON-протокол | ● | ● | ● | | ● | |
| **P2** Монолитный Payload | ● | ● | ● | | ● | |
| **P3** Единая очередь | **🔴** | | | | | |
| **P4** Full-file rewrite | | | **🔴** | | ● | |
| **P5** Глобальные static | **🔴** | | | ● | | |
| **P6** Polling events | ● | | | **🔴** | ● | |
| **P7** GroupChat !Send | ● | | | | | |
| **P8** Нет multi-device | | | | | | |
| **P9** Full-mesh | **🔴** | | | | | |
| **P10** Threading model | ● | | ● | | | |
| **P11** Нет blob transport | | **🔴** | | | ● | |
| **P12** Крипто-сложность | | | | | ● | **🔴** |
| **P13** Нет офлайн-очереди | | | **🔴** | | | |

🔴 = блокер (нельзя начать фазу без исправления)
● = серьёзное влияние (можно начать, но будет больно)

---

## 5. Рекомендуемый порядок исправлений

### Критические (блокеры H1)

1. **P5 — Сессии вместо static.** Ввести `Session`-объект. `RoomCall` →
   `HashMap<gid, RoomCall>`. Без этого H1 (несколько комнат) невозможен.

2. **P3 — Приоритизация очереди.** Разделить Event-очередь на приоритеты
   (calls > text > group > presence) или подписки. Без этого H1 (частые маячки
   убивают latency) будет мучительным.

3. **P9 — Audio-mesh + selective video.** Не нужен полноценный SFU — достаточно
   voice-activated video forwarding: только говорящий шлёт видео, остальные — audio
   или thumbnail.

### Важные (до H3)

4. **P4 — Хранилище.** Переход на SQLite или append-only WAL. Фундамент для H3
   (офлайн-очередь) и H5 (богатые сообщения).

5. **P10 — Threading model.** Tokio runtime или хотя бы structured concurrency.
   Без этого каждая новая фаза добавляет потоки хаотично.

6. **P11 — Blob transport.** Отдельный crate для chunked transfers. Фундамент
   для H2 (блобы) → H5 (файлы/картинки).

### Улучшения (по ходу)

7. **P1 + P2 — Бинарный протокол + extension fields.** Можно делать инкрементально:
   сначала binary envelope с JSON payload (совместимый путь), затем полный binary.

8. **P6 — Callback-модель вместо polling.** Совместно с H4 (UI overlay).

9. **P7 — Send-обёртка для GroupChat.** Push/pull через каналы (как CallEngine).

10. **P13 — Офлайн-очередь.** Требует P4 + P10. Реализуется в H3.

### Долгосрочные

11. **P8 — Multi-device.** Крупная рефакторинг-задача. Device keys + session keys +
    identity binding. Не блокирует H1–H5, но необходима для паритета с Telegram.

12. **P12 — Крипто-политика.** Документировать key hierarchy. Централизовать key
    derivation при добавлении новых механизмов.

---

## Резюме

Архитектура **хорошо работает для текущего функционала** (1:1 чат, звонки, группы
до 5-10 участников). Код чистый, модульный, крипто-аудит пройден.

**Главные bottlenecks для масштабирования:**

1. **Протокол и хранилище** (P1, P2, P4) — JSON overhead и full-file rewrite не
   выдержат роста объёма сообщений и типов контента. Это «фундамент» — исправлять
   рано, до H2/H3/H5.

2. **Модель сессий** (P5) — глобальные static блокируют несколько комнат,
   тестирование и эволюцию. Нужно первым для H1.

3. **Групповой звонок** (P9) — full-mesh не масштабируется. Но решение может быть
   простым: voice-activated video (только говорящий шлёт видео).

4. **Threading** (P7, P10) — без structured concurrency код станет неуправляемым
   при добавлении новых фоновых задач (блобы, офлайн-флуш, несколько комнат).

5. **Multi-device** (P8) — не блокирует ближайшие фазы, но это крупнейшая
   архитектурная щель по сравнению с Telegram/Signal. Требует отдельного дизайна.
