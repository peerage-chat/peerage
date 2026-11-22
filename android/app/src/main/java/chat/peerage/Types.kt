package chat.peerage

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

/**
 * Режим звука звонка (`settings/Media`): Авто — по каналу до 128 кбит, Голос — до 32,
 * Аудио — 128 в музыкальном режиме Opus. Предел канала движок накладывает сам
 * (`audio_bitrate` в room_call.rs): здесь только потолок и режим кодека.
 */
enum class AudioMode(val bitrate: Int, val music: Boolean) {
    AUTO(128_000, false),
    VOICE(32_000, false),
    MUSIC(128_000, true),
}

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
    /** Локальная заметка — как ЭТОТ пользователь назвал контакт; обычно ПУСТАЯ.
     *  Для показа — только [displayName]: отдельное имя у поля, чтобы его нельзя было
     *  взять по ошибке вместо имени (так экран входящего звонка остался без имени). */
    val localName: String,
    val fingerprint: String,
    val verified: Boolean,
    /** Когда сверен (unix-секунды); 0 — не сверен или сверен до того, как дату стали хранить. */
    val verifiedAt: Long = 0,
    /** Как сверен: "code" | "set"; пусто — неизвестно. */
    val verifiedHow: String = "",
    /** Послан запрос в контакты и ещё не принят (7i): наши сообщения ждут у нас. */
    val awaiting: Boolean = false,
    /** Самоназвание контакта (M2): как он назвал себя (ProfileUpdate). */
    val profileName: String = "",
    /** hex-хэш аватара контакта (M2); байты — через [AppModel.contactAvatars]. */
    val avatarId: String = "",
) {
    /** Имя для показа: локальная заметка → самоназвание → короткий hex.
     *  (docs/architecture: приоритет local → profile_name → hex.) */
    val displayName: String
        get() = localName.ifBlank { profileName.ifBlank { key.take(8) } }
}

/** Строка «Заблокированных» (5.2). [name] — как пир был показан в момент блокировки;
 *  пусто — незнакомец или контакт без имени. [contact] — был контактом. */
data class BlockedPeer(val key: String, val name: String, val contact: Boolean, val time: Long)

/** Входящий «запрос в контакты» от незнакомца (гейт вместо TOFU). Первое сообщение
 *  приходит сюда карточкой на одобрение, а не в чат. */
data class ContactRequest(
    val key: String,
    val fingerprint: String,
    /** Самоназвание из запроса — «указано отправителем», не проверено. */
    val name: String,
    /** Приветствие (шаг 7i): при принятии ляжет в чат первым сообщением. */
    val greeting: String,
    val time: Long,
    /** Контакт с похожим именем и другим ключом (7e): ключ и как он подписан; пусто — нет. */
    val lookalikeKey: String = "",
    val lookalikeName: String = "",
) {
    val shortKey: String get() = key.take(8)

    /** Как подписать запрос: самоназвание, а без него — начало ключа. */
    val shownName: String get() = name.ifBlank { shortKey }
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
    /** Упоминания поверх участков [text] (только группы), см. [Mention]. */
    val mentions: List<Mention> = emptyList(),
    /** Реакции под сообщением: эмодзи по первому появлению, у каждого — кто поставил. */
    val reactions: List<Reaction> = emptyList(),
) {
    /** Звонок в ленте личного чата (строку ведёт движок): исход в [text]. */
    val isCall: Boolean get() = kind == "call" || kind == "video_call"

    /** Отозвано «у всех»: на месте — строка «Сообщение удалено», содержимого нет. */
    val deleted: Boolean get() = kind == "deleted"

    /** То же надгробие, что кладёт движок: время, автор и место в ленте остаются. */
    fun retracted(): Message = Message(id, "", mine, time, status, kind = "deleted", sender = sender, row = row)
}

/**
 * Упоминание: участок текста `[at, at+len)` и ключ того, кого упомянули. Смещения в
 * БАЙТАХ UTF-8 — как их хранит и проверяет движок; в индексы Kotlin-строки (UTF-16)
 * их переводит [Mentions]. Рисуется участок как «@» + имя, которое ЭТО устройство
 * знает для ключа, а не как текст отправителя: подделать «@Аня» нельзя.
 */
@kotlinx.serialization.Serializable
data class Mention(val at: Int, val len: Int, val key: String)

/** Реакция под сообщением: эмодзи и кто её поставил (ключи, по порядку постановки). */
@kotlinx.serialization.Serializable
data class Reaction(val emoji: String, val who: List<String>)

/** Чат как адресат пересылки: личный (ключ собеседника) или группа (gid). */
data class ChatTarget(val id: String, val group: Boolean, val title: String = "")

/** Строка «По чатам» в «Хранилище»: чат и сколько его вложений на диске. */
data class StorageChat(val id: String, val group: Boolean, val files: Long, val bytes: Long)

/**
 * Меню у пузыря (долгое нажатие): сообщение, его чат и где пузырь лежит в окне — меню
 * встаёт рядом, а затемнение обходит пузырь по его форме.
 */
data class BubbleActions(
    val msg: Message,
    val chat: ChatTarget,
    val bounds: androidx.compose.ui.geometry.Rect,
    val shape: androidx.compose.ui.graphics.Shape,
)

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
/** Что за код обмена (экран «Добавить контакт»): status — "ok" | "invalid" | "self" | "known". */
data class CodeInfo(val status: String, val key: String, val name: String)

/** Набор сверки на пару: 12 знаков (hex) и 8 слов — одинаковые на обоих телефонах. */
data class PairFingerprint(val glyphs: String, val words: List<String>)

data class Me(val pubkey: String, val address: String, val fingerprint: String)

data class Group(
    val gid: String,
    val name: String,
    val members: Int,
    val admin: Boolean = false,
    /** Группа восстановлена из копии: история есть, членства нет — только чтение. */
    val archived: Boolean = false,
    /** Ключ админа (hex): метка «админ» в участниках. */
    val adminKey: String = "",
    /** Фото группы: hex блоба (пусто — нет); байты — [AppModel.groupPhoto]. */
    val photo: String = "",
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
    /** Упоминания в [lastText]: превью рисует их моими именами, как лента. */
    val lastMentions: List<Mention> = emptyList(),
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
    /** Адрес меша (выводит движок из ключа); пусто — соединение не поднялось. */
    val addr: String = "",
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

/** Источник флуда для UI: сколько отброшено с запуска и с каким темпом (ADR-007).
 *  Темп ноль — поток стих; объём остаётся. */
data class FloodPeer(
    val peer: String,
    val dropped: Long,
    val bytes: Long,
    val pps: Double,
    val bps: Long,
    /** Уже заблокирован — его пакеты режутся ещё в транспорте, до FFI. */
    val blocked: Boolean,
)

/** Ход смены адреса: обратный отсчёт паузы, затем итог. */
data class RotationState(
    /** Сколько секунд паузы осталось; 0 — пауза кончилась. */
    val secondsLeft: Int,
    /** Скольким отмеченным объявление уже ушло (считает движок). */
    val told: Int = 0,
    /** Сколько контактов отмечено. */
    val total: Int,
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
    val video: Boolean = false,
    /** Сколько звонков комнаты подряд свёрнуто в строку; 1 — одиночный. */
    val count: Int = 1,
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
