package dev.p2p.messenger

import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue

/**
 * Типы, которыми объясняются экраны: сообщение, контакт, группа, состояние сети.
 *
 * Отдельно от модели намеренно. `Model.kt` — это ПОВЕДЕНИЕ (что происходит при
 * отправке, докачке, входящем звонке), а здесь — словарь, на котором поведение
 * говорит. Раньше словарь и поведение лежали в одном файле на две с половиной
 * тысячи строк, и чтобы узнать, из чего состоит сообщение, приходилось листать
 * мимо звонков и резервных копий.
 */

data class VideoQuality(
    val label: String,
    val width: Int,
    val height: Int,
    val fps: Int,
    val bitrate: Int,
) {
    companion object {
        /** 720p 30 fps — есть у любой камеры. Список вариантов не хардкодим: он берётся
         *  из возможностей камеры ([CameraCaps]), иначе предлагали бы то, что она не умеет. */
        val default = VideoQuality("720p", 1280, 720, 30, 2_000_000)
    }
}

data class Contact(
    val key: String,
    /** Локальная заметка — как ЭТОТ пользователь назвал контакт (может быть пустой). */
    val name: String,
    val fingerprint: String,
    val verified: Boolean,
    /** Самоназвание контакта (M2): как он назвал себя (ProfileUpdate). */
    val profileName: String = "",
    /** hex-хэш аватара контакта (M2); байты — через [AppModel.contactAvatars]. */
    val avatarId: String = "",
) {
    /** Имя для показа: локальная заметка → самоназвание → короткий hex.
     *  (docs/architecture: приоритет local → profile_name → hex.) */
    val displayName: String
        get() = name.ifBlank { profileName.ifBlank { key.take(8) } }
}

/** Входящий «запрос в контакты» от незнакомца (гейт вместо TOFU). Первое сообщение
 *  приходит сюда карточкой на одобрение, а не в чат. */
data class ContactRequest(
    val key: String,
    val fingerprint: String,
    /** Превью первого сообщения (или «[image]»/«[file]»/«[voice]»). */
    val preview: String,
    val time: Long,
) {
    val shortKey: String get() = key.take(8)
}

data class Message(
    val id: Long,
    val text: String,
    val mine: Boolean,
    val time: Long,
    val status: String,
    /** Тип (M2): "text" | "image" | "file". */
    val kind: String = "text",
    /** hex-хэш блоба вложения (M2); байты — через Native.blobRead. */
    val blobId: String = "",
    val mime: String = "",
    /** Исходное имя файла вложения (M2). */
    val name: String = "",
    /** Размер вложения в байтах — виден в карточке ДО скачивания. */
    val size: Long = 0,
    /** Длительность аудио/видео в мс (0 — не применимо). */
    val durationMs: Int = 0,
    val width: Int = 0,
    val height: Int = 0,
    /** hex-хэш блоба-миниатюры (пусто — нет превью). */
    val thumbId: String = "",
    /** Скачано ли содержимое. Крупное не тянется само — сначала карточка. */
    val downloaded: Boolean = false,
    /** id сообщения, на которое это ответ (0 — обычное). Цитата рисуется из истории
     *  по ссылке (ADR-006), а не хранится копией текста. */
    val replyTo: Long = 0,
    /** hex автора: вместе с [id] — ссылка на сообщение. Читается одинаково в личном
     *  чате и в группе, поэтому «моё/чужое» больше не часть ключа. */
    val sender: String = "",
    /** hex автора цитируемого сообщения (пусто — обычное). */
    val replySender: String = "",
    /** Системная строка (смена состава группы). */
    val system: Boolean = false,
    /** hex автора ИСХОДНОГО сообщения, если это пересылка (пусто — своё). */
    val fwdFrom: String = "",
    /** Переслано, но автора не называем. */
    val fwdAnon: Boolean = false,
    /** Когда исправлено автором (0 — не редактировалось). */
    val editedAt: Long = 0,
    /** Курсор пагинации: номер строки в базе движка (0 — запись создана локально). */
    val row: Long = 0,
)

/** Чат как адресат пересылки: личный (ключ собеседника) или группа (gid). */
data class ChatTarget(val id: String, val group: Boolean, val title: String = "")

/**
 * Ссылка на строку истории: id сообщения плюс его автор (ADR-006). Пара, а не один
 * id: id посеян временем ОТПРАВИТЕЛЯ, поэтому у моего и чужого сообщения он может
 * совпасть.
 *
 * Автор годится и для личного чата, и для группы, поэтому ссылка одна на оба.
 * «Моё» выводится сравнением с собственным ключом. На проводе формы РАЗНЫЕ и свои
 * у каждого канала (`{id,mine}` в личном, `{id,sender}` в групповом) — перевод
 * живёт на границе JNI.
 */
data class MsgRef(val id: Long, val sender: String)

/** Идентичность пользователя (своя «визитка»). */
data class Me(val pubkey: String, val address: String, val fingerprint: String)

data class Group(
    val gid: String,
    val name: String,
    val members: Int,
    val admin: Boolean = false,
    /** Группа восстановлена из копии: история есть, членства нет — только чтение. */
    val archived: Boolean = false,
)

/**
 * Строка списка чатов на главной: последнее сообщение, время и локальные пометки.
 * Личные чаты и группы приходят одним списком — на главной они и живут вперемешку,
 * отсортированные по времени последнего сообщения.
 */
data class ChatSummary(
    /** hex: ключ собеседника или gid. */
    val id: String,
    val group: Boolean,
    /** 0 — не закреплён; иначе когда закрепили (позже — выше в списке). */
    val pinnedAt: Long = 0,
    val muted: Boolean = false,
    /** Превью последнего сообщения (текст или подпись вложения); пусто — чат пуст. */
    val lastText: String = "",
    val lastTime: Long = 0,
    val lastMine: Boolean = false,
    /** hex автора последнего сообщения — в группе показываем его имя перед текстом. */
    val lastSender: String = "",
    /** "text" | "image" | "file" | "voice" — определяет подпись вместо текста. */
    val lastKind: String = "text",
    /** Статус доставки СВОЕГО последнего сообщения ("" | sent | delivered | …). */
    val lastStatus: String = "",
    val lastSystem: Boolean = false,
) {
    val pinned: Boolean get() = pinnedAt > 0
}

/** Входящее приглашение в группу (карточка на одобрение вместо тихого авто-входа). */
data class GroupInvite(
    val gid: String,
    val name: String,
    val inviter: String,
    /** Мы уже согласились; ждём, когда пригласивший окажется в сети и ответит. */
    val waiting: Boolean = false,
)

/** Пир транспорта (адрес/направление/пинг/трафик) — из снапшота движка. */
data class NetPeer(
    val key: String,
    val inbound: Boolean,
    val up: Boolean,
    val latencyMs: Double,
    val rxRate: Long,
    val txRate: Long,
    val uptimeS: Double,
)

/** Снапшот стата сети (агрегаты считает движок). */
data class NetStats(
    val peerCount: Int,
    val tree: Int,
    val pingMs: Double,
    val rxRate: Long,
    val txRate: Long,
    val peers: List<NetPeer>,
)

/** Присутствие группы «N уч · M в сети» — агрегат (MLS-ростер ∩ presence) считает движок. */
data class GroupRoster(val total: Int, val online: Int)

/** Источник флуда для UI: сколько отброшено и с каким темпом (ADR-007). */
data class FloodPeer(
    val peer: String,
    val dropped: Long,
    val pps: Double,
    /** Уже заблокирован — его пакеты режутся ещё в транспорте, до FFI. */
    val blocked: Boolean,
)

/** Ход смены адреса: обратный отсчёт паузы, затем итог. */
data class RotationState(
    /** Сколько секунд паузы осталось; 0 — пауза кончилась. */
    val secondsLeft: Int,
    /** Скольким контактам уйдёт новый адрес. */
    val told: Int,
    /** Новый адрес — приходит событием после подъёма узла. */
    val newAddress: String = "",
)

/**
 * Пользовательская папка — это ВИД, а не коробка: чат может попадать сразу в
 * несколько. Живёт в prefs на клиенте: к протоколу отношения не имеет, собеседник
 * о ней ничего не знает.
 *
 * @param types типы чатов целиком ("dm" / "room")
 * @param chats конкретные чаты (ключ контакта или gid группы)
 * @param excludeRead прятать те, где нет непрочитанного
 */
data class Folder(
    val id: String,
    val name: String,
    val icon: String,
    val types: Set<String> = emptySet(),
    val chats: Set<String> = emptySet(),
    val excludeRead: Boolean = false,
)

/**
 * Запись журнала звонков. Живёт ТОЛЬКО на этом устройстве: наружу не уходит и ни с кем
 * не синхронизируется.
 *
 * @param outcome answered | missed | declined | unanswered
 * @param gid id комнаты; пусто у личного звонка
 */
data class CallEntry(
    val peer: String,
    val gid: String,
    val incoming: Boolean,
    val outcome: String,
    val started: Long,
    val duration: Long,
)

/** Строка очереди отправки: кому, сколько ждёт и с какого времени. */
data class OutboxPeer(val peer: String, val count: Int, val bytes: Long, val oldest: Long)

/** Входящий DM-звонок (до того как пользователь ответил или отклонил). */
data class IncomingRoomCall(
    val peer: String,
    val roomId: String,
    val audio: Boolean,
    val video: Boolean,
)

/** Участник комнаты-звонка (присутствие). */
data class RoomMember(val peer: String, val audio: Boolean, val video: Boolean)

/**
 * Состояние приложения поверх [Native].
 *
 * АРХИТЕКТУРА ПОТОКОВ (важно для отзывчивости): UI (главный поток Compose)
 * НИКОГДА не вызывает `Native.*` напрямую — любой JNI-вызов может заблокировать
 * (сеть/Go-FFI/диск/мьютексы ядра). Всё взаимодействие с ядром — на выделенных
 * фоновых потоках:
 *   • [pollThread]   — опрос событий ядра;
 *   • [tickerThread] — периодический статус (пиры/онлайн);
 *   • [ops]          — single-thread executor для разовых операций (отправка,
 *                      контакты, звонки, группы), сериализует JNI.
 * Состояние — Compose snapshot-state (mutableStateOf/List/Map), его БЕЗОПАСНО
 * писать из фоновых потоков: рекомпозиция планируется на главный поток сама.
 * Так главный поток не блокируется ничем → нет ANR.
 */
