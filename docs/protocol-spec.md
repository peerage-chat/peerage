# Контракт API движка (Command / Event / Snapshot) — спека v0

**Статус:** черновик, отражает ТЕКУЩУЮ поверхность (2026-06-27) · версия контракта `0`.
Связано: [ADR-003](adr/ADR-003-engine-api.md) (решение), [v1.0-scope](v1.0-scope.md).

> Цель: единственный контракт между движком (Rust) и клиентом (Android/JNI,
> десктоп). Клиент рендерит **Snapshot** и шлёт **Command**; движок асинхронно
> отдаёт **Event**. Домейн-логику клиент НЕ считает. Это формализация уже
> существующих ~40 JNI-вызовов как типизированного контракта; миграция —
> инкрементально (см. ADR-003), первый пример типизации — `NetStats`.

## Принципы
- **Три рода обмена:** `Command` (намерение, может вернуть подтверждение),
  `Query→Snapshot` (синхронное чтение состояния), `Event` (асинхронное уведомление).
- **Версионирование:** поле `protocol_version` (сейчас `0`); эволюция аддитивная —
  новые варианты/поля не ломают старых клиентов.
- **Медиа — отдельный горячий канал.** Поток PCM/H264 (`room_push_*`/`room_pull_*`)
  НЕ заворачивается в типизированный Command/Event (производительность); это
  низкоуровневый byte/short-канал по контракту «push/pull». Описан отдельно.
- **Идентификаторы:** ключи/gid/room_id — hex ed25519/32 байта; время — unix-секунды
  (косметика), тайминги корректности — монотонные внутри движка.

## Commands (намерение клиента → движок)
Группировка по подсистемам. `→ T` — синхронный возврат.

**Жизненный цикл**
- `Start{data_dir, vault_key} → Me{pubkey,address,fingerprint}` — инициализация сессии.
- `Stop` · `DestroySession` — остановка/освобождение.
- `PollEvent → Event?` — выбрать следующее событие (до перехода на push).

**Контакты (TOFU)**
- `AddContact{key,name} → {added|known|error}`
- `RenameContact{key,name}` · `RemoveContact{key}` · `SetVerified{key,bool}`
- `Connect{key}` — держать фоновое соединение.

**Сообщения 1:1**
- `SendDm{key,text} → msg_id` (≥0 принято к доставке; <0 пир недостижим → офлайн-
  очередь, **репатриация в M3**).

**Группы (MLS)**
- `GroupCreate{name} → bool` · `GroupInvite{gid,key}` · `GroupSendText{gid,text}`

**Комнаты-звонки (присутствие)**
- `RoomInviteDm{key,audio,video} → room_id` · `RoomJoinDm{key,…} → room_id` ·
  `RoomDeclineDm{key}`
- `RoomJoin{gid,audio,video}` · `RoomLeave{gid}` · `RoomSetMuted{bool}`

**Сеть**
- `ReconnectPeers` — форс-реконнект публичных пиров (обход backoff).
- `SetRelay{bool}` *(план M3)* — режим «узел сети».

**Диагностика**
- `BenchRecvStart` · `BenchBlast{key,kb} → stats`

## Snapshots (синхронное чтение состояния)
Движок отдаёт ГОТОВОЕ состояние (агрегаты считает он, не клиент):
- `Me` — {pubkey, address, fingerprint}.
- `Contacts` — [{key,name,fingerprint,verified}].
- `History{key}` — [{id,text,mine,time,status}] (incl. queued-плейсхолдеры).
- `IsOnline{key} → bool`.
- **`NetStats`** — {peer_count, tree, ping_ms, rx_rate, tx_rate, peers:[{key,inbound,
  up,latency_ms,rx_rate,tx_rate,uptime_s}]}. ✅ типизировано (первый пример контракта).
- `Groups` — [{gid,name,members}] · `GroupHistory{gid}` — [{text,mine,time,sender,system}]
  · `GroupMembers{gid}` — [key…].
- **`GroupPresence{gid}`** — {total, online}. ✅ типизировано (агрегат «N уч · M в
  сети» = MLS-ростер ∩ presence, считает движок; презенс динамичен → на момент чтения).
- `RoomParticipants{gid}` / `RoomPresencePeers{gid}` — [{peer,audio,video}].
- `OfflineQueue` *(план M3)* — {count, per_peer…}.

## Events (асинхронно движок → клиент)
- `Presence{peer,online}` — изменение присутствия (триггерит флаш офлайн-очереди).
- `Text{peer,id,text,time}` — входящее 1:1.
- `Ack{peer,id}` / `Undelivered{peer,id}` — статус доставки исходящего (5-мин ретрай
  истёк → Undelivered → офлайн-очередь, **M3**).
- `RoomInvite{peer,room_id,audio,video}` / `RoomDecline{peer,room_id}`
- `RoomPresence{gid,peer,flags|left}` — состав комнаты на лету.
- `GroupsChanged` — состав/список групп изменился (перечитать снапшоты).
- `GroupMsg{gid,text,sender,system}` — входящее групповое (incl. системные строки).

## Медиа — горячий канал (вне типизированного контракта)
Контракт «push/pull» поверх byte/short-буферов, минимум оверхеда:
- Аудио: `RoomAudioStart{gid}→{rate,ch,frame}` · `RoomAudioStop` ·
  `RoomPushAudio{pcm[]}` · `RoomPullAudio→pcm[]`.
- Видео: `RoomPushVideo{au,keyframe}` · `RoomPullVideoPacket{peer}→bytes` ·
  `RoomSetVideoCap{frames}`.
Фрагментация/джиттер/микс — внутри движка (`media`/`androidjni room_call`).

## Анти-abuse (ingress)
- ✅ **Механизм (M1):** receiver-enforced token-bucket на источник
  (`p2p_core::ratelimit::RateLimiter`) в приёмном цикле мессенджера. Применяется по
  Yggdrasil src-ключу **до расшифровки** → флуд дропается до дорогого крипто.
  Раздельные бюджеты: строгий для CHAN_CHAT (спам-вектор), щедрый для CHAN_MEDIA
  (звонок легитимно высокочастотен). Навязывает принимающий → модификация
  отправителя не обходит. Юнит-тесты без устройства. Для меш-источника, который
  нельзя отключить на линк-уровне, исчерпание бюджета = «разрыв сессии» на входе.
- ⏳ **Градуировка по доверию (M3):** чужие — общая мелкая корзина (Sybil-
  устойчивость) + локальная репутация + storage-cap. См. v1.0-scope «Анти-abuse».

## Статус миграции (ADR-003)
- ✅ `NetStats` — первый типизированный снапшот (libygg `ygg_peers_json` → движок
  считает агрегаты → клиент рендерит).
- ✅ `GroupPresence` — агрегат «N уч · M в сети» репатриирован в движок (раньше
  Kotlin пересекал ростер ∩ online сам); клиент удалил `groupMemberKeys`.
- ✅ **Крейт `p2p-engine` вынесен**: вся device-independent доменная логика
  (`Engine` = бывш. Session, event_pump, обработчики, `GroupManager`,
  `RoomCallEngine`, агрегаты) живёт там; `androidjni` — тонкий JNI-кодек без
  домейн-логики. Движок юнит-тестируется без устройства (агрегаты сети, презенс-TTL,
  презенс групп, rate-limit).
- ✅ **Ingress rate-limit** (см. «Анти-abuse»).
- ⏳ Репатриировать: офлайн-очередь+TTL (**M3**, сцеплено с message-model S7),
  настройки.
