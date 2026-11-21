package chat.peerage

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Зеркало схемы движка (`engine/src/dto.rs` и типы `peerage_core::store`).
 *
 * **Зачем.** Раньше клиент разбирал ответы движка по строковым ключам
 * (`o.optString("blob_id")`), и расхождение с Rust обнаруживалось на устройстве:
 * компилятор про эти строки ничего не знает. Здесь схема — типы, и опечатка
 * ловится сборкой.
 *
 * **Правило при правках:** поле меняется ОДНОВРЕМЕННО здесь и в `dto.rs`. В Rust
 * форму провода держат тесты (`dto::tests`), здесь — `ignoreUnknownKeys`, чтобы
 * новое поле со стороны движка не роняло старый клиент.
 */

/** Единый парсер: неизвестные поля игнорируем, отсутствующие берём из значений
 *  по умолчанию — движок может добавить поле раньше, чем клиент про него узнает. */
internal val EngineJson = Json {
    ignoreUnknownKeys = true
    coerceInputValues = true
    explicitNulls = false
}

/**
 * Сериализация ИСХОДЯЩЕГО в движок. Отдельно от [EngineJson] ровно из-за одного
 * флага: по умолчанию kotlinx НЕ пишет поля, равные значению по умолчанию, — и
 * состав резервной копии уезжал в ядро пустым `{}`, то есть «ничего не класть».
 * Найдено живым прогоном, а не тестом, потому что обе стороны по отдельности
 * работали правильно.
 *
 * Любой DTO, уходящий наружу, сериализуется ЧЕРЕЗ ЭТУ точку.
 */
internal val WireJson = Json { encodeDefaults = true }

// ── Снимки ──────────────────────────────────────────────────────────────────

@Serializable
internal data class ContactDto(
    val key: String = "",
    val name: String = "",
    val fingerprint: String = "",
    val added: Long = 0,
    val verified: Boolean = false,
    @SerialName("verified_at") val verifiedAt: Long = 0,
    @SerialName("verified_how") val verifiedHow: String? = null,
    val awaiting: Boolean = false,
    @SerialName("profile_name") val profileName: String = "",
    @SerialName("avatar_id") val avatarId: String = "",
    val helpful: Long = 0,
)

@Serializable
internal data class MessageDto(
    val id: Long = 0,
    /** hex автора: ссылка на сообщение — пара «автор + id» (ADR-006). */
    val sender: String = "",
    val text: String = "",
    val mine: Boolean = false,
    val time: Long = 0,
    val status: String = "",
    val kind: String = "text",
    @SerialName("blob_id") val blobId: String = "",
    val mime: String = "",
    val name: String = "",
    val size: Long = 0,
    @SerialName("duration_ms") val durationMs: Int = 0,
    @SerialName("reply_to") val replyTo: Long = 0,
    /** hex автора цитируемого сообщения (пусто — обычное). */
    @SerialName("reply_sender") val replySender: String = "",
    /** Системная строка (смена состава группы) — рисуется не пузырём. */
    val system: Boolean = false,
    /** hex автора ИСХОДНОГО сообщения, если это пересылка (пусто — своё). */
    @SerialName("fwd_from") val fwdFrom: String = "",
    /** Переслано, но автора не называем (пересылающий скрыл источник). */
    @SerialName("fwd_anon") val fwdAnon: Boolean = false,
    /** Когда исправлено автором (0 — не редактировалось). */
    @SerialName("edited_at") val editedAt: Long = 0,
    /** Курсор пагинации: номер строки в базе движка. Для догрузки истории вверх. */
    val row: Long = 0,
    val width: Int = 0,
    val height: Int = 0,
    @SerialName("thumb_id") val thumbId: String = "",
    val downloaded: Boolean = false,
    val mentions: List<Mention> = emptyList(),
    val reactions: List<Reaction> = emptyList(),
)

/**
 * Строка списка чатов из движка: где чат, локальные пометки и последнее сообщение.
 * `last = null` — чат без переписки (например, закреплённый пустой).
 */
@Serializable
internal data class ChatSummaryDto(
    val chat: String = "",
    val group: Boolean = false,
    /** 0 — не закреплён; иначе время закрепления (позже закрепили — выше). */
    @SerialName("pinned_at") val pinnedAt: Long = 0,
    val muted: Boolean = false,
    val last: MessageDto? = null,
)

/** Находка поиска: строка истории плюс чат, где она лежит. */
@Serializable
internal data class SearchHitDto(
    val chat: String = "",
    val group: Boolean = false,
    val id: Long = 0,
    val sender: String = "",
    val text: String = "",
    val mine: Boolean = false,
    val time: Long = 0,
    val kind: String = "text",
    /** Строка в базе: курсор окна истории вокруг находки. */
    val row: Long = 0,
)

/** Страница поиска в чате: всего находок и эта страница (свежие первыми). */
@Serializable
internal data class SearchPageDto(
    val total: Int = 0,
    val hits: List<MessageDto> = emptyList(),
)

@Serializable
internal data class ContactRequestDto(
    val key: String = "",
    val fingerprint: String = "",
    val name: String = "",
    val greeting: String = "",
    val time: Long = 0,
    /** Контакт, на чьё имя похоже самоназвание запроса (7e); null — не похоже. */
    val lookalike: LookalikeDto? = null,
)

@Serializable
internal data class LookalikeDto(
    val key: String = "",
    val name: String = "",
)

@Serializable
internal data class GroupRowDto(
    val gid: String = "",
    val name: String = "",
    val members: Int = 0,
    val admin: Boolean = false,
    /** Ключ админа группы (hex; пусто — неизвестен). */
    @SerialName("admin_key") val adminKey: String = "",
    /** Фото группы: hex блоба (пусто — нет). */
    val photo: String = "",
    /** Мы не состоим в группе: история из копии, писать нельзя. */
    val archived: Boolean = false,
)

@Serializable
internal data class GroupInviteDto(
    val gid: String = "",
    val name: String = "",
    val inviter: String = "",
    /** «Вступить» нажато, ждём ответа пригласившего (он мог быть офлайн). */
    val waiting: Boolean = false,
)

@Serializable
internal data class RoomMemberDto(
    val peer: String = "",
    val audio: Boolean = false,
    val video: Boolean = false,
)

@Serializable
internal data class OutboxStatsDto(
    val pending: Int = 0,
    val bytes: Long = 0,
    val peers: Int = 0,
    val warn: Boolean = false,
)

@Serializable
internal data class OutboxPeerDto(
    val peer: String = "",
    val count: Int = 0,
    val bytes: Long = 0,
    val oldest: Long = 0,
)

@Serializable
internal data class ChatStorageDto(
    val peer: String = "",
    val files: Long = 0,
    val bytes: Long = 0,
)

@Serializable
internal data class StorageStatsDto(
    val blobs: Long = 0,
    @SerialName("blob_bytes") val blobBytes: Long = 0,
    @SerialName("outbox_pending") val outboxPending: Long = 0,
    @SerialName("outbox_bytes") val outboxBytes: Long = 0,
    @SerialName("by_kind") val byKind: Map<String, Long> = emptyMap(),
    val chats: List<ChatStorageDto> = emptyList(),
)

/**
 * Состав резервной копии данных: и оценка ДО создания, и итог после (что реально
 * добавлено при восстановлении). Один тип на оба случая — цифры те же самые.
 */
@Serializable
internal data class BackupStatsDto(
    val contacts: Long = 0,
    val messages: Long = 0,
    @SerialName("group_messages") val groupMessages: Long = 0,
    val notes: Long = 0,
    val calls: Long = 0,
    val attachments: Long = 0,
    @SerialName("attachment_bytes") val attachmentBytes: Long = 0,
    /** Вложения крупнее лимита кадра — в копию не попадут (говорим числом). */
    @SerialName("skipped_big") val skippedBig: Long = 0,
    /** Непусто — операция не удалась, текст показывается пользователю. */
    val error: String = "",
)

/** Что положить в копию. Поля совпадают с `peerage_engine::backup::Parts`. */
@Serializable
internal data class BackupPartsDto(
    val contacts: Boolean = true,
    val messages: Boolean = true,
    /** Групповая переписка. Членство не восстанавливается — только история. */
    val groups: Boolean = true,
    val notes: Boolean = true,
    val calls: Boolean = false,
    val attachments: Boolean = false,
)

/** Строка блок-листа: `blocked_json` движка. */
@Serializable
internal data class BlockedPeerDto(
    val key: String = "",
    val name: String = "",
    val contact: Boolean = false,
    val time: Long = 0,
)

/** Источник флуда: сколько его пакетов отброшено и с каким темпом. */
@Serializable
internal data class FloodPeerDto(
    val peer: String = "",
    val dropped: Long = 0,
    val bytes: Long = 0,
    val pps: Double = 0.0,
    val bps: Long = 0,
    val blocked: Boolean = false,
)

/** Ход смены адреса: скольким из отмеченных уже ушло объявление. */
@Serializable
internal data class RotationProgressDto(val told: Int = 0, val total: Int = 0)

@Serializable
internal data class CallEntryDto(
    val peer: String = "",
    val gid: String = "",
    val incoming: Boolean = false,
    val outcome: String = "",
    val started: Long = 0,
    val duration: Long = 0,
)

@Serializable
internal data class CallParamsDto(
    @SerialName("call_id") val callId: Int = 0,
    @SerialName("sample_rate") val sampleRate: Int = 0,
    val channels: Int = 0,
    @SerialName("frame_len") val frameLen: Int = 0,
)

@Serializable
internal data class NetPeerDto(
    val key: String = "",
    val inbound: Boolean = false,
    val up: Boolean = false,
    @SerialName("latency_ms") val latencyMs: Double = 0.0,
    @SerialName("rx_rate") val rxRate: Long = 0,
    @SerialName("tx_rate") val txRate: Long = 0,
    @SerialName("uptime_s") val uptimeS: Double = 0.0,
    val addr: String = "",
)

@Serializable
internal data class NetStatsDto(
    @SerialName("peer_count") val peerCount: Int = 0,
    val tree: Int = 0,
    @SerialName("ping_ms") val pingMs: Double = 0.0,
    @SerialName("rx_rate") val rxRate: Long = 0,
    @SerialName("tx_rate") val txRate: Long = 0,
    val peers: List<NetPeerDto> = emptyList(),
)

/** «N участников · M в сети» — агрегат считает движок, клиент только рисует. */
@Serializable
internal data class GroupRosterDto(val total: Int = 1, val online: Int = 1)

/** Ответ `Native.start`: либо сессия, либо человекочитаемая ошибка. */
@Serializable
internal data class StartResultDto(
    @SerialName("session_ptr") val sessionPtr: Long = 0,
    val pubkey: String = "",
    val address: String = "",
    val fingerprint: String = "",
    val error: String? = null,
)

/** Диагностический замер (обе стороны отдают mbps). */
@Serializable
internal data class BenchDto(
    val mbps: Double = 0.0,
    @SerialName("sent_pkts") val sentPkts: Long = 0,
    @SerialName("sent_bytes") val sentBytes: Long = 0,
    val secs: Double = 0.0,
)

// ── Преобразование в модели UI ──────────────────────────────────────────────
// DTO повторяют схему движка, модели — то, чем удобно рисовать. Разделение
// намеренное: переименование поля в протоколе не должно течь в composable'ы.

internal fun ContactDto.toModel() = Contact(
    key = key,
    localName = name,
    fingerprint = fingerprint,
    verified = verified,
    verifiedAt = verifiedAt,
    verifiedHow = verifiedHow.orEmpty(),
    awaiting = awaiting,
    profileName = profileName,
    avatarId = avatarId,
)

internal fun MessageDto.toModel() = Message(
    id = id,
    text = text,
    mine = mine,
    time = time,
    status = status,
    // Пустой kind — «неизвестно», а не «вложение»: старые записи с пустым полем
    // рисовались дважды (как текст и как подпись к вложению).
    kind = kind.ifEmpty { "text" },
    blobId = blobId,
    mime = mime,
    name = name,
    size = size,
    durationMs = durationMs,
    replyTo = replyTo,
    sender = sender,
    replySender = replySender,
    system = system,
    fwdFrom = fwdFrom,
    fwdAnon = fwdAnon,
    editedAt = editedAt,
    row = row,
    width = width,
    height = height,
    thumbId = thumbId,
    downloaded = downloaded,
    mentions = mentions,
    reactions = reactions,
)

// ── События ─────────────────────────────────────────────────────────────────

/**
 * Событие движка. Разбирается по полю `type`, как и раньше, но набор полей у
 * каждого варианта теперь фиксирован типом.
 *
 * Неизвестный `type` попадает в [Unknown] вместо исключения: движок может начать
 * слать новое событие раньше, чем клиент научится его показывать, и это не повод
 * ронять поток разбора.
 */
@Serializable
internal sealed interface EngineEvent {
    @Serializable
    @SerialName("presence")
    data class Presence(val peer: String, val online: Boolean) : EngineEvent

    /**
     * Новая строка переписки — личной (`chat` = ключ собеседника) или групповой
     * (`chat` = gid), своей или входящей. Ровно та строка, что легла в историю:
     * подпись, «переслано», ответ и курсор пагинации приезжают сразу.
     */
    @Serializable
    @SerialName("message")
    data class MessageEv(val chat: String, val group: Boolean = false, val msg: MessageDto) : EngineEvent

    /** Собеседник отозвал своё сообщение — строку надо убрать из ленты. */
    @Serializable
    @SerialName("retracted")
    data class Retracted(val peer: String, val id: Long) : EngineEvent

    /** В группе кто-то отозвал сообщение — историю перечитать целиком. */
    @Serializable
    @SerialName("group_retracted")
    data class GroupRetracted(val gid: String) : EngineEvent

    @Serializable
    @SerialName("ack")
    data class Ack(val peer: String, val id: Long) : EngineEvent

    @Serializable
    @SerialName("undelivered")
    data class Undelivered(val peer: String, val id: Long) : EngineEvent

    /** Собеседник прочитал наши сообщения: у этих id статус теперь «read» (2.10). */
    @Serializable
    @SerialName("read")
    data class Read(val peer: String, val ids: List<Long>) : EngineEvent

    /** Собеседник печатает (2.10). Гаснет сам — см. [AppModel.typingUntil]. */
    @Serializable
    @SerialName("typing")
    data class Typing(val peer: String) : EngineEvent

    /** Своё сообщение выброшено из досылки (срок, сброс очереди) — «не доставлено». */
    @Serializable
    @SerialName("failed")
    data class Failed(val peer: String, val id: Long) : EngineEvent

    @Serializable
    @SerialName("contact_request")
    data class ContactRequestEv(val peer: String, val new: Boolean = false) : EngineEvent

    @Serializable
    @SerialName("contacts_changed")
    data object ContactsChanged : EngineEvent

    @Serializable
    @SerialName("groups_changed")
    data object GroupsChanged : EngineEvent

    @Serializable
    @SerialName("group_invites_changed")
    data object GroupInvitesChanged : EngineEvent

    /** Список закреплённых в чате изменился (hex собеседника или gid). */
    @Serializable
    @SerialName("pins_changed")
    data class PinsChanged(val chat: String) : EngineEvent

    /** Реакции под сообщением `(id, sender)` изменились: новый набор целиком. */
    @Serializable
    @SerialName("reactions")
    data class Reactions(
        val chat: String,
        val group: Boolean = false,
        val id: Long,
        val sender: String,
        val reactions: List<Reaction> = emptyList(),
    ) : EngineEvent

    /** Сообщение исправлено автором — заменить текст в ленте на месте. */
    @Serializable
    @SerialName("message_edited")
    data class MessageEdited(
        val chat: String,
        val id: Long,
        val sender: String,
        val text: String,
        val mentions: List<Mention> = emptyList(),
    ) : EngineEvent

    @Serializable
    @SerialName("group_removed")
    data class GroupRemoved(val gid: String) : EngineEvent

    @Serializable
    @SerialName("room_invite")
    data class RoomInvite(
        val peer: String,
        @SerialName("room_id") val roomId: String,
        val audio: Boolean = false,
        val video: Boolean = false,
    ) : EngineEvent

    @Serializable
    @SerialName("room_decline")
    data class RoomDecline(val peer: String, @SerialName("room_id") val roomId: String) : EngineEvent

    @Serializable
    @SerialName("room_presence")
    data class RoomPresence(
        val gid: String,
        val peer: String,
        val flags: Int = 0,
        val left: Boolean = false,
    ) : EngineEvent

    /** Наш адрес сменился: личность прежняя, меняется только адрес. */
    @Serializable
    @SerialName("transport_rotated")
    data class TransportRotated(val address: String, val epoch: Int) : EngineEvent

    /** Смена адреса не удалась: адрес прежний; `online` — вернулись ли на него. */
    @Serializable
    @SerialName("transport_rotate_failed")
    data class TransportRotateFailed(val online: Boolean = false) : EngineEvent

    @Serializable
    @SerialName("blob_received")
    data class BlobReceived(val id: String, val mime: String = "", val size: Long = 0) : EngineEvent

    @Serializable
    @SerialName("blob_progress")
    data class BlobProgress(val id: String, val done: Int = 0, val total: Int = 0) : EngineEvent

    /** Докачка не началась: `too_large` или `no_space`. Заказ снят движком. */
    @Serializable
    @SerialName("blob_failed")
    data class BlobFailed(val id: String, val reason: String = "") : EngineEvent

    /** Событие, которого этот клиент ещё не знает. Тег служебный: движок его не
     *  шлёт, вариант нужен только чтобы разбор не бросал исключение. */
    @Serializable
    @SerialName("__unknown")
    data object Unknown : EngineEvent
}

/** Разобрать событие. Неизвестный тип и битый JSON — [EngineEvent.Unknown]. */
internal fun parseEvent(json: String): EngineEvent =
    runCatching { EngineJson.decodeFromString<EngineEvent>(json) }.getOrDefault(EngineEvent.Unknown)

/** Разобрать список; битый ответ — пустой список, а не падение UI. */
/** Переключатели «Приватности» (2.10): оба по умолчанию выключены, взаимны. */
@Serializable
data class PrivacyDto(
    @SerialName("read_receipts") val readReceipts: Boolean = false,
    val typing: Boolean = false,
)

internal inline fun <reified T> decodeList(json: String): List<T> =
    runCatching { EngineJson.decodeFromString<List<T>>(json) }.getOrDefault(emptyList())

/** Разобрать объект; битый ответ — null. */
internal inline fun <reified T> decodeOrNull(json: String): T? =
    runCatching { EngineJson.decodeFromString<T>(json) }.getOrNull()
