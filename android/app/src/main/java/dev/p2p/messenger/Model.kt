package dev.p2p.messenger

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.toMutableStateList
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

/** Пресет качества видео-звонка. */
data class VideoQuality(
    val label: String,
    val width: Int,
    val height: Int,
    val fps: Int,
    val bitrate: Int,
) {
    val key get() = "${width}x$height@$fps"

    companion object {
        val presets = listOf(
            VideoQuality("480p  30fps", 854, 480, 30, 1_000_000),
            VideoQuality("720p  30fps", 1280, 720, 30, 2_000_000),
            VideoQuality("720p  60fps", 1280, 720, 60, 4_000_000),
            VideoQuality("1080p 30fps", 1920, 1080, 30, 4_000_000),
            VideoQuality("1080p 60fps", 1920, 1080, 60, 8_000_000),
        )
        val default get() = presets[1] // 720p 30fps
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
    /** Курсор пагинации: номер строки в базе движка (0 — запись создана локально). */
    val row: Long = 0,
)

/** Чат как адресат пересылки: личный (ключ собеседника) или группа (gid). */
data class ChatTarget(val id: String, val group: Boolean, val title: String = "")

/**
 * Ссылка на строку истории: id сообщения + чьё оно. Пара, а не один id: id посеян
 * временем ОТПРАВИТЕЛЯ, поэтому у моего и чужого сообщения он может совпасть.
 * Тот же формат читает движок (`deleteMessages`).
 */
@kotlinx.serialization.Serializable
data class MsgRef(val id: Long, val mine: Boolean)

/**
 * Ссылка на групповое сообщение: id + автор. В группе собеседников много, поэтому
 * (в отличие от DM) флага «моё/чужое» недостаточно.
 */
@kotlinx.serialization.Serializable
data class GroupMsgRef(val id: Long, val sender: String)

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

data class GroupMsg(
    /** id в пределах ОТПРАВИТЕЛЯ; вместе с [sender] — ссылка на сообщение
     *  (ADR-006). 0 — сообщение старого клиента: сослаться не на что. */
    val id: Long = 0,
    /** Курсор пагинации: номер строки в базе движка (0 — запись создана локально). */
    val row: Long = 0,
    /** Ссылка ответа: id цитируемого (0 — обычное) и hex его автора. */
    val replyTo: Long = 0,
    val replySender: String = "",
    val text: String,
    val mine: Boolean,
    val time: Long,
    /** hex ed25519-pubkey отправителя (имя/цвет/аватарка в UI). "" — неизвестно. */
    val sender: String = "",
    /** Системная строка (смена состава) — рендерится отдельно. */
    val system: Boolean = false,
    /** hex автора исходного сообщения, если это пересылка (пусто — своё). */
    val fwdFrom: String = "",
    /** Переслано, но автора не называем. */
    val fwdAnon: Boolean = false,
    /** Тип (M2): "text" | "image" | "voice" | "file". */
    val kind: String = "text",
    /** hex-хэш блоба вложения (M2); байты — через AppModel.mediaBlobs. */
    val blobId: String = "",
    val mime: String = "",
    val name: String = "",
    /** Метаданные карточки: известны ДО загрузки байт (имя/размер/превью). */
    val size: Long = 0,
    val durationMs: Int = 0,
    val width: Int = 0,
    val height: Int = 0,
    val thumbId: String = "",
    /** Байты вложения уже у нас (своё сообщение или докачано). */
    val downloaded: Boolean = false,
)

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
class AppModel private constructor() {
    companion object {
        /**
         * Пауза офлайн при смене адреса, секунды. Зашита, а не настраивается:
         * величина не про вкус, а про то, за сколько старый ключ выпадает из дерева
         * маршрутизации. Замерено — хватает ~30 с; 60 взято с запасом.
         */
        const val ROTATE_GAP_SECS = 60

        /**
         * С какого темпа считаем происходящее флудом, достойным уведомления.
         * Ниже — это шум одиночных отказов (пир не в контактах, пара пакетов),
         * дёргать им пользователя незачем.
         */
        const val FLOOD_NOTIFY_PPS = 20.0

        /** Единственный экземпляр на процесс. Нативный узел/потоки тоже одни на
         *  процесс, поэтому модель должна переживать пересоздание Activity (иначе
         *  утекают второй узел и дублирующие poll/ticker-циклы → потеря событий). */
        val instance: AppModel by lazy { AppModel() }

        /** Докачка заказана, но чанки ещё не пошли (пир офлайн / идёт манифест). */
        const val DOWNLOAD_INIT = -1f

        /** Сколько недавних эмодзи помним (ряд на 8 + запас на вытеснение). */
        const val RECENT_EMOJI_MAX = 24
    }

    var me by mutableStateOf<Me?>(null)
    var error by mutableStateOf<String?>(null)
    var sessionPtr: Long = 0L

    /** Секретная сид-фраза (24 слова) — раскрывается по запросу (онбординг M4
     *  «Показать фразу»), СЕКРЕТ. null = скрыта. Читается через [ops], не с UI-потока. */
    var revealedPhrase by mutableStateOf<String?>(null)
        private set

    fun revealSeedPhrase() = ops.execute { revealedPhrase = Native.seedPhrase(sessionPtr) }

    /** Записал ли пользователь резервную копию ключа (M4). Локальный факт, не
     *  секрет: без него человек не узнает, что личность держится на одной фразе. */
    var keyBackupDone by mutableStateOf(false)
        private set

    /** Отметить, что фраза записана (нажал «Записал»). */
    fun markKeyBackupDone() {
        keyBackupDone = true
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("key_backup_done", true)?.apply()
    }

    fun clearRevealedPhrase() {
        revealedPhrase = null
    }

    /** Импорт личности из фразы (M4): валидирует+пишет profile.json на [ops], результат
     *  на главном потоке. true → успех (вызывающий перезапускает процесс). */
    fun importIdentity(dataDir: String, phrase: String, onResult: (Boolean) -> Unit) = ops.execute {
        val ok = Native.importSeedPhrase(dataDir, phrase)
        android.os.Handler(android.os.Looper.getMainLooper()).post { onResult(ok) }
    }

    /** Свой аватар (M2), байты изображения; null = не задан. Читается через [ops]. */
    var selfAvatar by mutableStateOf<ByteArray?>(null)
        private set

    /** Своё самоназвание (M2). Живёт в движке (рассылается контактам), не в prefs. */
    var selfName by mutableStateOf("")
        private set

    fun loadSelfAvatar() = ops.execute { selfAvatar = Native.selfAvatar(sessionPtr) }

    fun setSelfAvatar(content: ByteArray, mime: String) = ops.execute {
        Native.setSelfAvatar(sessionPtr, content, mime)
        selfAvatar = Native.selfAvatar(sessionPtr)
    }

    /** Задать своё имя (M2): движок сохранит и разошлёт контактам. */
    fun updateSelfName(name: String) = ops.execute {
        Native.setSelfName(sessionPtr, name.trim())
        selfName = Native.selfName(sessionPtr)
    }

    /** Байты аватаров контактов по hex-хэшу (M2). Заполняется при обновлении контактов;
     *  UI читает [Contact.avatarId] → сюда. */
    val contactAvatars = mutableStateMapOf<String, ByteArray>()

    /** Байты вложений чата по hex-хэшу блоба (M2). Реактивный кэш: при докачке
     *  (blob_received) картинка появляется в пузыре без ручного рефреша. */
    val mediaBlobs = mutableStateMapOf<String, ByteArray>()

    /** Подтянуть байты блоба в кэш, если он уже есть локально (на [ops]). */
    private fun cacheBlob(blobId: String) {
        if (blobId.isNotEmpty() && !mediaBlobs.containsKey(blobId)) {
            Native.blobRead(sessionPtr, blobId).takeIf { it.isNotEmpty() }?.let { mediaBlobs[blobId] = it }
        }
    }

    val contacts = mutableStateListOf<Contact>()

    /** Входящие запросы в контакты от незнакомцев (гейт). */
    val contactRequests = mutableStateListOf<ContactRequest>()
    val online = mutableStateMapOf<String, Boolean>()
    val unread = mutableStateMapOf<String, Int>()
    val messages = mutableStateMapOf<String, SnapshotStateList<Message>>()

    var currentChat by mutableStateOf<String?>(null)
    var netPeers by mutableStateOf(0)

    /** Снапшот стата сети (экран «Сеть»). Обновляется тикером на переднем плане. */
    var netStats by mutableStateOf<NetStats?>(null)

    /** Режим «узел сети» (ретрансляция чужого трафика). Пока только UI-флаг —
     *  привязка к движку (вкл/выкл relay) — TODO, когда появится нативный вызов. */
    var relayEnabled by mutableStateOf(true)

    /** Результат последней попытки добавить контакт ("" | added | known | error). */
    var addStatus by mutableStateOf("")

    // Диагностика: тест пропускной способности меша.
    var benchRecv by mutableStateOf(false) // режим приёмника включён
    var benchRecvMbps by mutableStateOf(0.0) // живая скорость приёма
    var benchResult by mutableStateOf("") // результат отправки (для blast)

    // Группы (MLS).
    val groups = mutableStateListOf<Group>()

    /** Входящие приглашения в группы (карточки на одобрение). */
    val groupInvites = mutableStateListOf<GroupInvite>()
    val groupMessages = mutableStateMapOf<String, SnapshotStateList<GroupMsg>>()
    var currentGroup by mutableStateOf<String?>(null)

    /** Присутствие открытой группы «N уч · M в сети» — агрегат отдаёт движок
     *  (groupPresence). Обновляется тикером, пока групповой чат открыт. */
    var currentGroupRoster by mutableStateOf<GroupRoster?>(null)

    // Комната-звонок (Discord-style присутствие). gid комнаты, где мы сейчас.
    var currentRoom by mutableStateOf<String?>(null)
    val roomMembers = mutableStateListOf<RoomMember>()

    /** Hex-ключи говорящих сейчас участников (active-speaker рамка). Опрашивается
     *  отдельным потоком ~4 раза/с, пока мы в звонке. */
    var speakingPeers by mutableStateOf<Set<String>>(emptySet())
        private set

    /** Скорость трафика (kbps) по участникам для тех-инфо: hex участника → входящий от
     *  него; "self" → свой исходящий. Считается по дельте байт между опросами поллера. */
    var peerKbps by mutableStateOf<Map<String, Int>>(emptyMap())
        private set

    /** Угол поворота входящего видео на участника (град, угол сенсора отправителя).
     *  UI доворачивает тайл. Энкодер шлёт сырой кадр с сенсора → нужен доворот. */
    val roomMemberRotation = mutableStateMapOf<String, Int>()

    /** Живые участники каждой группы по маячкам — обновляется тикером.
     *  Заполняется даже когда мы сами не в комнате (peer слал нам RoomPresence). */
    val groupPresence = mutableStateMapOf<String, List<RoomMember>>()

    // Качество видео — загружается из SharedPreferences при start().
    var videoQuality by mutableStateOf(VideoQuality.default)
        private set

    fun changeVideoQuality(q: VideoQuality) {
        videoQuality = q
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putString("video_quality", q.key)?.apply()
    }

    /** Тех-инфо оверлей в звонке (пинг/разрешение/fps/битрейт на тайлах). Тумблер в ⚙️. */
    var showTechInfo by mutableStateOf(false)
        private set

    fun toggleTechInfo() {
        showTechInfo = !showTechInfo
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("tech_info", showTechInfo)?.apply()
    }

    /** Предупреждать о разросшейся очереди недоставленного (вкл по умолчанию).
     *  Сообщения при этом НЕ удаляются — это только уведомление. Тумблер в ⚙️. */
    var warnOutboxGrowth by mutableStateOf(true)
        private set

    fun toggleOutboxWarning() {
        warnOutboxGrowth = !warnOutboxGrowth
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("warn_outbox", warnOutboxGrowth)?.apply()
    }

    /** Сколько сообщений ждёт доставки и надо ли предупредить (из движка). */
    var outboxPending by mutableStateOf(0)
        private set
    var outboxWarn by mutableStateOf(false)
        private set

    private fun refreshOutboxStats() {
        val s = decodeOrNull<OutboxStatsDto>(Native.outboxStats(sessionPtr)) ?: return
        outboxPending = s.pending
        outboxWarn = s.warn
    }

    // ── Хранилище (простой экран; нормальный дизайн — позже) ──────────────────
    var storageBlobs by mutableStateOf(0L)
        private set
    var storageBlobBytes by mutableStateOf(0L)
        private set
    var storageOutboxPending by mutableStateOf(0L)
        private set

    /** Что занимает место, по типам вложения: kind → байты. */
    var storageByKind by mutableStateOf<List<Pair<String, Long>>>(emptyList())
        private set

    /** По чатам: (ключ пира, файлов, байт), самый тяжёлый первым. */
    var storageChats by mutableStateOf<List<Triple<String, Long, Long>>>(emptyList())
        private set

    fun refreshStorage() = ops.execute {
        val st = decodeOrNull<StorageStatsDto>(Native.storageStats(sessionPtr)) ?: return@execute
        storageBlobs = st.blobs
        storageBlobBytes = st.blobBytes
        storageOutboxPending = st.outboxPending
        // Самый тяжёлый тип первым — полоса состава читается сверху вниз.
        storageByKind = st.byKind.toList().sortedByDescending { it.second }
        storageChats = st.chats.map { Triple(it.peer, it.files, it.bytes) }
    }

    // ── Резервная копия ДАННЫХ (M4) ───────────────────────────────────────────
    // Не путать с копией КЛЮЧА ([revealSeedPhrase]): ключ возвращает личность, копия
    // данных — переписку.

    /** Когда сделана последняя копия данных (unix-секунды); 0 — не делалась.
     *  Локальный факт, как и [keyBackupDone]: сам файл мы после сохранения не видим
     *  (он у пользователя), а сказать «копии нет» после её создания — врать. */
    var dataBackupAt by mutableStateOf(0L)
        private set

    private fun markDataBackupDone() {
        val now = System.currentTimeMillis() / 1000
        dataBackupAt = now
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putLong("data_backup_at", now)?.apply()
    }

    /** Что и сколько войдёт в копию. Считается по базе, поэтому через [ops]. */
    internal var backupEstimate by mutableStateOf(BackupStatsDto())
        private set

    fun refreshBackupEstimate() = ops.execute {
        backupEstimate = decodeOrNull<BackupStatsDto>(Native.backupEstimate(sessionPtr))
            ?: BackupStatsDto()
    }

    /**
     * Создать копию в файл `path`. **Блокирующая** (Argon2id ~секунда плюс вложения) —
     * звать с фонового потока; UI на это время показывает прогресс.
     */
    internal fun exportBackupBlocking(path: String, parts: BackupPartsDto, password: String): BackupStatsDto {
        // Только через WireJson: см. его док — иначе поля с дефолтными значениями
        // молча не доедут до движка.
        val json = WireJson.encodeToString(BackupPartsDto.serializer(), parts)
        val out = Native.backupExport(sessionPtr, path, json, password)
        val stats = decodeOrNull<BackupStatsDto>(out)
            ?: BackupStatsDto(error = "не удалось создать копию")
        if (stats.error.isEmpty()) markDataBackupDone()
        return stats
    }

    /** Восстановить из копии. Возвращает ДОБАВЛЕННОЕ (не «что было в файле»). */
    internal fun importBackupBlocking(path: String, password: String): BackupStatsDto {
        val out = Native.backupImport(sessionPtr, path, password)
        val stats = decodeOrNull<BackupStatsDto>(out)
            ?: BackupStatsDto(error = "не удалось прочитать копию")
        if (stats.error.isEmpty()) {
            // Контакты и чаты поменялись — перечитываем то, что уже на экране.
            refreshContacts()
            refreshStorage()
        }
        return stats
    }

    /** Очередь по получателям: (ключ, сколько ждёт, байт, время самой старой записи). */
    var outboxByPeer by mutableStateOf<List<OutboxPeer>>(emptyList())
        private set

    fun refreshOutbox() = ops.execute {
        outboxByPeer = decodeList<OutboxPeerDto>(Native.outboxByPeer(sessionPtr))
            .map { OutboxPeer(it.peer, it.count, it.bytes, it.oldest) }
        outboxPending = outboxByPeer.sumOf { it.count }
    }

    /** Сбросить очередь одному получателю (после подтверждения в UI). */
    fun clearOutboxFor(peer: String) = ops.execute {
        Native.clearOutboxFor(sessionPtr, peer)
        refreshOutbox()
    }

    /** Сбросить очередь всем (после двойного подтверждения в UI). */
    fun clearOutboxAll() = ops.execute {
        Native.clearOutboxAll(sessionPtr)
        refreshOutbox()
    }

    /** Удалить сохранённые вложения. История и ссылки остаются — байты можно
     *  скачать у собеседника заново (блобы контент-адресные). */
    fun clearAttachments() = ops.execute {
        Native.clearBlobs(sessionPtr)
        mediaBlobs.clear()
        contactAvatars.clear()
        refreshStorage()
    }

    /** Заметки себе («Сохранённое»), старые первыми. */
    val notes = mutableStateListOf<Message>()

    fun refreshNotes() = ops.execute {
        val list = decodeList<MessageDto>(Native.notes(sessionPtr)).map { it.toModel() }
        notes.clear()
        notes.addAll(list)
        atChatStart -= "notes"
    }

    /**
     * Догрузить заметки ВЫШЕ показанных. «Сохранённое» — тот же чат (с самим собой),
     * поэтому и страницы те же: движок отдаёт хвост, остальное просим по прокрутке.
     */
    fun loadOlderNotes(done: (Int) -> Unit) {
        val meKey = me?.pubkey
        val top = notes.firstOrNull { it.row != 0L }
        if (meKey == null || top == null || "notes" in atChatStart || !loadingOlder.add("notes")) {
            done(0)
            return
        }
        ops.execute {
            try {
                val page = decodeList<MessageDto>(
                    Native.historyPage(sessionPtr, meKey, top.time, top.row),
                ).map { it.toModel() }
                if (page.isEmpty()) atChatStart += "notes"
                if (page.isNotEmpty()) notes.addAll(0, page)
                done(page.size)
            } finally {
                loadingOlder -= "notes"
            }
        }
    }

    fun saveNote(text: String) = ops.execute {
        Native.saveNote(sessionPtr, text)
        refreshNotes()
    }

    fun deleteNote(id: Long) = ops.execute {
        Native.deleteNote(sessionPtr, id)
        refreshNotes()
    }

    /** Журнал звонков (свежие первыми). */
    val callLog = mutableStateListOf<CallEntry>()

    fun refreshCallLog() = ops.execute {
        val list = decodeList<CallEntryDto>(Native.callLog(sessionPtr, 200))
            .map { CallEntry(it.peer, it.gid, it.incoming, it.outcome, it.started, it.duration) }
        callLog.clear()
        callLog.addAll(list)
    }

    fun clearCallLog() = ops.execute {
        Native.clearCallLog(sessionPtr)
        refreshCallLog()
    }

    /** Визуальные отпечатки ключей (hex) — кэш, чтобы не дёргать JNI на рекомпозицию. */
    private val fpCache = mutableMapOf<String, String>()

    /** Отпечаток для сверки глазами. Пустой ключ — свой. Считает движок (одна
     *  реализация на все клиенты), UI только рисует. */
    fun visualFingerprint(key: String = ""): String = fpCache.getOrPut(key) {
        if (sessionPtr == 0L) "" else runCatching { Native.visualFingerprint(sessionPtr, key) }.getOrDefault("")
    }

    // ── Пользовательские папки ──────────────────────────────────────────────
    val folders = mutableStateListOf<Folder>()

    private fun saveFolders() {
        val arr = JSONArray()
        folders.forEach { f ->
            arr.put(
                JSONObject()
                    .put("id", f.id)
                    .put("name", f.name)
                    .put("icon", f.icon)
                    .put("types", JSONArray(f.types.toList()))
                    .put("chats", JSONArray(f.chats.toList()))
                    .put("exclude_read", f.excludeRead),
            )
        }
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putString("folders", arr.toString())?.apply()
    }

    private fun loadFolders() {
        val raw = appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
            ?.getString("folders", null) ?: return
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return
        folders.clear()
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            fun strSet(name: String): Set<String> {
                val a = o.optJSONArray(name) ?: return emptySet()
                return (0 until a.length()).mapNotNull { a.optString(it).takeIf { s -> s.isNotEmpty() } }.toSet()
            }
            folders += Folder(
                id = o.optString("id"),
                name = o.optString("name"),
                icon = o.optString("icon"),
                types = strSet("types"),
                chats = strSet("chats"),
                excludeRead = o.optBoolean("exclude_read", false),
            )
        }
    }

    fun saveFolder(f: Folder) {
        val i = folders.indexOfFirst { it.id == f.id }
        if (i >= 0) folders[i] = f else folders += f
        saveFolders()
    }

    fun deleteFolder(id: String) {
        folders.removeAll { it.id == id }
        saveFolders()
    }

    fun folderById(id: String): Folder? = folders.firstOrNull { it.id == id }

    /** Попадает ли личный чат в папку. */
    fun folderHasDm(f: Folder, key: String): Boolean =
        ("dm" in f.types || key in f.chats) && !(f.excludeRead && (unread[key] ?: 0) == 0)

    /** Попадает ли группа/комната в папку. */
    fun folderHasGroup(f: Folder, gid: String): Boolean = "room" in f.types || gid in f.chats

    /** Недавно использованные эмодзи (первым — самый свежий). Панель показывает их
     *  сверху: в ходу у человека десяток символов, ради них не должно быть выбора
     *  категории. Живут в prefs, а не в движке — это предпочтение клиента. */
    var recentEmoji by mutableStateOf<List<String>>(emptyList())
        private set

    fun useEmoji(e: String) {
        recentEmoji = (listOf(e) + recentEmoji.filter { it != e }).take(RECENT_EMOJI_MAX)
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putString("recent_emoji", recentEmoji.joinToString(" "))?.apply()
    }

    private fun loadSettings() {
        val prefs = appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE) ?: return
        showTechInfo = prefs.getBoolean("tech_info", false)
        recentEmoji = prefs.getString("recent_emoji", "")?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
        warnOutboxGrowth = prefs.getBoolean("warn_outbox", true)
        val key = prefs.getString("video_quality", null) ?: return
        videoQuality = VideoQuality.presets.find { it.key == key } ?: VideoQuality.default
    }

    // ── Экран шаринга видео (долгий тап «Видео» в звонке). ──────────────────────
    var showVideoShare by mutableStateOf(false)
        private set
    var shareScreenMode by mutableStateOf(false) // false = камера, true = экран
        private set
    var shareFront by mutableStateOf(true)
        private set
    var shareResW by mutableStateOf(1280)
        private set
    var shareResH by mutableStateOf(720)
        private set
    var shareFps by mutableStateOf(30)
        private set
    var shareBitrateAuto by mutableStateOf(true)
        private set

    /** Индекс ручного битрейта (0 ниже · 1 рекомендация · 2 выше) — ОТНОСИТЕЛЬНЫЙ:
     *  переживает смену разрешения/fps (для 4К берётся свой вариант того же уровня). */
    var shareBitrateIdx by mutableStateOf(1)
        private set

    /** Рекомендуемый битрейт (бит/с) ≈ w·h·fps·0.08 (H264, ~0.08 bpp/кадр). */
    fun autoBitrate(w: Int, h: Int, fps: Int): Int =
        (w.toLong() * h * fps * 0.08).toInt().coerceIn(1_000_000, 40_000_000)

    /** Три варианта битрейта (бит/с) под текущее разрешение/fps: ниже · рекоменд. · выше. */
    fun shareBitrateOptions(): List<Int> {
        val a = autoBitrate(shareResW, shareResH, shareFps)
        return listOf(a * 6 / 10, a, a * 3 / 2).map { (it / 1_000_000).coerceAtLeast(1) * 1_000_000 }
    }

    /** Применяемый битрейт: рекомендация (idx 1) при «Авто», иначе выбранный вариант. */
    fun shareBitrateBps(): Int = shareBitrateOptions()[if (shareBitrateAuto) 1 else shareBitrateIdx]

    fun openVideoShare() {
        shareScreenMode = false
        shareFront = cameraFront
        shareResW = videoQuality.width
        shareResH = videoQuality.height
        shareFps = videoQuality.fps
        shareBitrateAuto = true
        shareBitrateIdx = 1
        showVideoShare = true
    }

    fun closeVideoShare() {
        showVideoShare = false
    }

    fun pickSource(screen: Boolean) {
        shareScreenMode = screen
    }

    fun pickFacing(front: Boolean) {
        shareFront = front
    }

    fun pickRes(w: Int, h: Int) {
        shareResW = w
        shareResH = h // битрейт следует сам: авто=рекоменд, ручной=тот же idx под новое разрешение
    }

    fun pickFps(f: Int) {
        shareFps = f
    }

    fun pickBitrateAuto() {
        shareBitrateAuto = true
    }

    fun pickBitrateIdx(i: Int) {
        shareBitrateIdx = i.coerceIn(0, 2)
        shareBitrateAuto = false
    }

    /** Применить выбор и начать показ видео с этими параметрами. */
    fun applyVideoShare() {
        showVideoShare = false
        if (shareScreenMode) return // шаринг экрана (MediaProjection) — отдельная фича, пока no-op
        cameraFront = shareFront
        changeVideoQuality(VideoQuality("${shareResH}p", shareResW, shareResH, shareFps, shareBitrateBps()))
        if (videoActive) {
            // видео уже идёт → пересобрать отправитель с новыми параметрами
            stopVideoSender()
            maybeStartVideoSender()
        } else {
            toggleVideo()
        }
    }

    // ── Настройки звука (долгий тап «Микрофон» в звонке). ───────────────────────
    var showAudioSettings by mutableStateOf(false)
        private set

    /** Вывод звонка: "builtin" (встроенный/разговорный), "speaker" (громкая), "bluetooth". */
    var audioOut by mutableStateOf("builtin")
        private set

    /** Вход (микрофон): "builtin", "headset" (проводная гарнитура), "bluetooth". */
    var audioIn by mutableStateOf("builtin")
        private set

    /** Аппаратная обработка микрофона (AEC/NS/AGC). */
    var audioFx by mutableStateOf(AudioFx())
        private set

    /** Качество: авто (адаптивно) либо ручной Opus-битрейт 16–48 кбит. */
    var audioQualityAuto by mutableStateOf(true)
        private set
    var audioBitrate by mutableStateOf(24_000)
        private set

    /** Живой уровень микрофона 0..1 — индикатор на экране настроек. */
    var micLevel by mutableStateOf(0f)
        private set

    /** Для CallSettings: «громкая связь» = вывод на динамик. */
    val speakerOn get() = audioOut == "speaker"

    fun toggleSpeaker() = pickAudioOut(if (audioOut == "speaker") "builtin" else "speaker")

    fun openAudioSettings() {
        showAudioSettings = true
    }

    fun closeAudioSettings() {
        showAudioSettings = false
        micLevel = 0f
    }

    fun pickAudioOut(dev: String) {
        audioOut = dev
        applyOutputDevice()
        saveAudioPrefs()
    }

    fun pickAudioIn(dev: String) {
        audioIn = dev
        saveAudioPrefs() // вход применяется при следующем старте аудио-сессии (preferredDevice)
    }

    fun setAudioFx(ns: Boolean = audioFx.ns, aec: Boolean = audioFx.aec, agc: Boolean = audioFx.agc) {
        audioFx = AudioFx(ns, aec, agc)
        saveAudioPrefs() // эффекты привязаны к audioSessionId → применяются на следующем старте
    }

    fun setAudioQuality(auto: Boolean, bitrate: Int = audioBitrate) {
        audioQualityAuto = auto
        audioBitrate = bitrate.coerceIn(16_000, 48_000)
        saveAudioPrefs() // TODO: Opus bitrate/mode на сессию — нужен JNI-сеттер (+ adaptive)
    }

    @Suppress("DEPRECATION")
    private fun applyOutputDevice() {
        val am = appContext?.getSystemService(android.content.Context.AUDIO_SERVICE)
            as? android.media.AudioManager ?: return
        when (audioOut) {
            "speaker" -> {
                runCatching { am.stopBluetoothSco() }
                am.isBluetoothScoOn = false
                am.isSpeakerphoneOn = true
            }
            "bluetooth" -> {
                am.isSpeakerphoneOn = false
                runCatching { am.startBluetoothSco() }
                am.isBluetoothScoOn = true
            }
            else -> { // встроенный (разговорный)
                runCatching { am.stopBluetoothSco() }
                am.isBluetoothScoOn = false
                am.isSpeakerphoneOn = false
            }
        }
    }

    /** Доступные выходы: встроенный/динамик всегда; bluetooth — если подключён. */
    fun availableAudioOuts(): Set<String> {
        val am = appContext?.getSystemService(android.content.Context.AUDIO_SERVICE)
            as? android.media.AudioManager ?: return setOf("builtin", "speaker")
        val s = linkedSetOf("builtin", "speaker")
        if (hasBluetoothAudio(am)) s += "bluetooth"
        return s
    }

    /** Доступные входы: встроенный всегда; проводная гарнитура/bluetooth — если есть. */
    fun availableAudioIns(): Set<String> {
        val am = appContext?.getSystemService(android.content.Context.AUDIO_SERVICE)
            as? android.media.AudioManager ?: return setOf("builtin")
        val s = linkedSetOf("builtin")
        val devs = am.getDevices(android.media.AudioManager.GET_DEVICES_INPUTS)
        if (devs.any { it.type == android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET }) s += "headset"
        if (hasBluetoothAudio(am)) s += "bluetooth"
        return s
    }

    private fun hasBluetoothAudio(am: android.media.AudioManager): Boolean =
        am.getDevices(android.media.AudioManager.GET_DEVICES_OUTPUTS).any {
            it.type == android.media.AudioDeviceInfo.TYPE_BLUETOOTH_SCO ||
                it.type == android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP
        }

    /** AudioDeviceInfo выбранного входа (для CallAudio.preferredDevice); null = дефолт. */
    private fun selectedInputDevice(): android.media.AudioDeviceInfo? {
        if (audioIn == "builtin") return null
        val am = appContext?.getSystemService(android.content.Context.AUDIO_SERVICE)
            as? android.media.AudioManager ?: return null
        val type = when (audioIn) {
            "headset" -> android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET
            "bluetooth" -> android.media.AudioDeviceInfo.TYPE_BLUETOOTH_SCO
            else -> return null
        }
        return am.getDevices(android.media.AudioManager.GET_DEVICES_INPUTS).firstOrNull { it.type == type }
    }

    private fun saveAudioPrefs() {
        appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putString("audio_out", audioOut)
            ?.putString("audio_in", audioIn)
            ?.putBoolean("fx_ns", audioFx.ns)
            ?.putBoolean("fx_aec", audioFx.aec)
            ?.putBoolean("fx_agc", audioFx.agc)
            ?.putBoolean("aq_auto", audioQualityAuto)
            ?.putInt("aq_bitrate", audioBitrate)
            ?.apply()
    }

    private fun loadAudioPrefs() {
        val p = appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE) ?: return
        audioOut = p.getString("audio_out", "builtin") ?: "builtin"
        audioIn = p.getString("audio_in", "builtin") ?: "builtin"
        audioFx = AudioFx(p.getBoolean("fx_ns", true), p.getBoolean("fx_aec", true), p.getBoolean("fx_agc", true))
        audioQualityAuto = p.getBoolean("aq_auto", true)
        audioBitrate = p.getInt("aq_bitrate", 24_000)
    }

    var appContext: android.content.Context? = null

    /** Приложение на переднем плане? Ставит MainActivity (onStart/onStop). В фоне
     *  poll/ticker троттлятся — меньше пробуждений CPU при удержанном wakelock. */
    @Volatile var foreground = true
    var incomingCall by mutableStateOf<IncomingRoomCall?>(null)
    var dmCallPeer by mutableStateOf<String?>(null)
    var videoActive by mutableStateOf(false)

    @Volatile private var roomAudio: CallAudio? = null

    @Volatile private var videoSender: VideoSender? = null

    /** Приёмники видео per-peer: тайл сетки даёт surface → приёмник на этого пира.
     *  Стабильны: создаются при появлении surface тайла, гасятся при уходе (участник
     *  вышел / тайл скрыт). key(peer) в Compose не даёт пересоздавать surface на
     *  перерисовке → нет тёрки декодеров. */
    private val videoReceivers = java.util.concurrent.ConcurrentHashMap<String, VideoReceiver>()

    /** Surface тайла «Вы» для локального превью с камеры (без энкодера). */
    @Volatile private var selfPreviewSurface: android.view.Surface? = null

    /** Микрофон заглушён (управляется из UI звонка). */
    var muted by mutableStateOf(false)

    /** Текущий объектив: фронтальная камера (true) или тыловая. Влияет на зеркало
     *  превью «Вы» (селфи-зеркало только у фронталки). */
    var cameraFront by mutableStateOf(true)
        private set

    // Фоновое взаимодействие с ядром (никогда не на главном потоке).
    private val ops = Executors.newSingleThreadExecutor { Thread(it, "native-ops").apply { isDaemon = true } }

    @Volatile private var running = false
    private var pollThread: Thread? = null
    private var tickerThread: Thread? = null
    private var netCallback: ConnectivityManager.NetworkCallback? = null

    @Volatile private var started = false

    // ── Офлайн-очередь сообщений ─────────────────────────────────────────────
    // peer → [(text, unix-timestamp)]. Сохраняется в offline_queue.json (TTL 7 дней).
    // Пишется только из ops/pollThread под queueLock; читается оттуда же.
    private val queueLock = Any()
    private val offlineQueueMap = HashMap<String, MutableList<Pair<String, Long>>>()
    private var queueFile: File? = null

    @Volatile private var localMsgId = -1L // отрицательные ID для «не отправлено» в логе

    /** Инициализация ядра. Звать с фонового потока (см. MainActivity).
     *  Идемпотентно: при пересоздании Activity повторно НЕ инициализирует. */
    /**
     * Инициализация ядра. `password` нужен на уровнях at-rest PASSWORD/BOTH.
     * Возвращает false, если пароль не подошёл, — вызывающий показывает ошибку, а
     * не стартует с мусорным ключом (иначе база выглядела бы повреждённой).
     */
    fun start(dataDir: String, password: String? = null): Boolean {
        if (started) return true
        val vaultKey = runCatching { VaultKeyManager.getOrCreate(File(dataDir), password) }
            .getOrElse { return false }
        started = true
        loadSettings()
        loadAudioPrefs()
        val started = decodeOrNull<StartResultDto>(Native.start(dataDir, vaultKey))
        if (started == null || started.error != null || started.sessionPtr == 0L) {
            error = started?.error ?: "engine start failed"
            return false
        }
        sessionPtr = started.sessionPtr
        me = Me(started.pubkey, started.address, started.fingerprint)
        transportEpoch = Native.transportEpoch(sessionPtr)
        val prefs = appContext
            ?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
        keyBackupDone = prefs?.getBoolean("key_backup_done", false) ?: false
        dataBackupAt = prefs?.getLong("data_backup_at", 0L) ?: 0L
        refreshContactsNow()
        refreshRequestsNow()
        refreshGroupsNow()
        refreshGroupInvitesNow()
        loadWanted() // докачки, заказанные до перезапуска
        loadFolders()
        selfAvatar = Native.selfAvatar(sessionPtr) // M2: свой аватар (уже на этом потоке)
        selfName = Native.selfName(sessionPtr) // M2: своё имя из движка
        migrateSelfNameFromPrefs(dataDir) // разовый перенос имени из старых prefs в движок
        queueFile = File(dataDir, "offline_queue.json")
        loadOfflineQueue()
        restoreQueuedToLog()
        startLoops()
        registerNetworkCallback()
        return true
    }

    /** Разовая миграция (M2): раньше имя жило только в SharedPreferences и не
     *  рассылалось. Если движок пустой, а в prefs имя есть — переносим в движок
     *  (он сохранит и разошлёт), затем чистим prefs, чтобы не повторяться. */
    private fun migrateSelfNameFromPrefs(dataDir: String) {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences("p2p_settings", Context.MODE_PRIVATE)
        val old = prefs.getString("profile_name", "").orEmpty().trim()
        if (selfName.isBlank() && old.isNotBlank()) {
            Native.setSelfName(sessionPtr, old)
            selfName = Native.selfName(sessionPtr)
        }
        if (old.isNotBlank()) prefs.edit().remove("profile_name").apply()
    }

    // Мгновенный реконнект при восстановлении сети (до срабатывания тикера).
    private fun registerNetworkCallback() {
        val ctx = appContext ?: return
        val cm = ctx.getSystemService(ConnectivityManager::class.java) ?: return
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                val ptr = sessionPtr
                if (ptr != 0L) {
                    android.util.Log.i("p2pnet", "Network available — forcing peer reconnect")
                    runCatching { Native.reconnectPeers(ptr) }
                }
            }
        }
        netCallback = cb
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                cm.registerDefaultNetworkCallback(cb)
            } else {
                val req = NetworkRequest.Builder()
                    .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                    .build()
                cm.registerNetworkCallback(req, cb)
            }
        }
    }

    private fun startLoops() {
        if (running) return
        running = true
        pollThread = Thread {
            while (running) {
                var e = Native.pollEvent(sessionPtr)
                while (e.isNotEmpty()) {
                    runCatching { applyEvent(e) }
                    e = Native.pollEvent(sessionPtr)
                }
                // В фоне реже опрашиваем (входящие задержатся максимум на интервал —
                // приемлемо), на переднем плане — отзывчиво.
                Thread.sleep(if (foreground) 80 else 400)
            }
        }.apply {
            isDaemon = true
            name = "native-poll"
            start()
        }
        tickerThread = Thread {
            var tick = 0L
            var zeroPeerTicks = 0 // счётчик тиков без пиров
            while (running) {
                runCatching {
                    val p = Native.netPeers(sessionPtr)
                    val t = Native.netTree(sessionPtr)
                    netPeers = p
                    contacts.map { it.key }.forEach { key ->
                        val wasOnline = online[key]
                        val nowOnline = Native.isOnline(sessionPtr, key)
                        online[key] = nowOnline
                        // При первом тике (wasOnline=null) или переходе в онлайн — флашим очередь.
                        if (nowOnline && wasOnline != true) flushQueueFor(key)
                    }
                    // Флуд и офлайн — дёшево и нужно даже в фоне: пользователь
                    // платит за чужой поток прямо сейчас.
                    val wasOffline = offline
                    offline = Native.isOffline(sessionPtr)
                    appContext?.let { ctx ->
                        if (offline && !wasOffline) FloodNotifier.showOffline(ctx)
                        if (!offline && wasOffline) FloodNotifier.cancelOffline(ctx)
                    }
                    val flood = decodeList<FloodPeerDto>(Native.floodPeers(sessionPtr))
                        .map { FloodPeer(it.peer, it.dropped, it.pps, it.blocked) }
                    if (floodPeers != flood) {
                        floodPeers.clear()
                        floodPeers.addAll(flood)
                    }
                    // Уведомление — ОДИН раз на эпизод: иначе оно само станет вторым
                    // флудом. Эпизод считается законченным, когда поток стих.
                    val top = flood.maxByOrNull { it.pps }
                    if (top != null && top.pps >= FLOOD_NOTIFY_PPS) {
                        if (!floodNotified) {
                            floodNotified = true
                            appContext?.let { ctx ->
                                val who = contacts.firstOrNull { it.key == top.peer }?.displayName
                                    ?: (top.peer.take(8) + "…")
                                FloodNotifier.showFlood(ctx, who, top.pps.toInt())
                            }
                        }
                    } else if (flood.isEmpty()) {
                        floodNotified = false
                    }
                    if (benchRecv) {
                        benchRecvMbps = decodeOrNull<BenchDto>(Native.benchRecvStats(sessionPtr))?.mbps ?: 0.0
                    }
                    // Очередь недоставленного: раз в ~10 тиков хватает (ничего не
                    // теряется, это только повод показать предупреждение).
                    if (tick % 10 == 0L) refreshOutboxStats()
                    // Снапшот стата сети — только на переднем плане (экран «Сеть»).
                    if (foreground) {
                        decodeOrNull<NetStatsDto>(Native.netStats(sessionPtr))?.let { n ->
                            netStats = NetStats(
                                n.peerCount,
                                n.tree,
                                n.pingMs,
                                n.rxRate,
                                n.txRate,
                                n.peers.map {
                                    NetPeer(it.key, it.inbound, it.up, it.latencyMs, it.rxRate, it.txRate, it.uptimeS)
                                },
                            )
                        }
                    }
                    // Присутствие открытой группы «N уч · M в сети» — агрегат
                    // (ростер ∩ онлайн) считает движок, клиент лишь рендерит.
                    currentGroup?.let { gid ->
                        decodeOrNull<GroupRosterDto>(Native.groupPresence(sessionPtr, gid))?.let {
                            currentGroupRoster = GroupRoster(it.total, it.online)
                        }
                    }
                    // Присутствие в комнате: кто сейчас внутри (по свежим маячкам).
                    currentRoom?.let { gid ->
                        val list = decodeList<RoomMemberDto>(Native.roomParticipants(sessionPtr, gid))
                            .map { RoomMember(it.peer, it.audio, it.video) }
                        // Только при реальном изменении состава/флагов — иначе каждый тик
                        // (2с в звонке) форсит рекомпозицию всего экрана → рывок видео.
                        if (roomMembers != list) {
                            roomMembers.clear()
                            roomMembers.addAll(list)
                        }
                    }
                    // Присутствие в группах, к которым мы НЕ подключены:
                    // показывает кто уже в комнате до того как мы вошли.
                    groups.forEach { g ->
                        if (g.gid == currentRoom) return@forEach
                        val list = decodeList<RoomMemberDto>(Native.roomPresencePeers(sessionPtr, g.gid))
                            .map { RoomMember(it.peer, it.audio, it.video) }
                        if (groupPresence[g.gid] != list) groupPresence[g.gid] = list
                    }
                    // Диагностика + авто-реконнект меша.
                    // peers=0 два тика подряд (~6с) → Yggdrasil потерял все пиры.
                    // Принудительно re-add чтобы обойти внутренний backoff.
                    if (p == 0) {
                        zeroPeerTicks++
                        if (zeroPeerTicks >= 2) {
                            android.util.Log.w("p2pnet", "peers=0 for $zeroPeerTicks ticks — forcing peer reconnect")
                            Native.reconnectPeers(sessionPtr)
                            zeroPeerTicks = 0
                        }
                    } else {
                        zeroPeerTicks = 0
                    }
                    android.util.Log.i("p2pnet", "tick=$tick peers=$p tree=$t incoming=${incomingCall != null} room=${currentRoom != null}")
                }
                tick++
                // В звонке (currentRoom) держим присутствие свежим (2с) даже в фоне;
                // иначе на переднем плане 3с, в фоне 15с — реже будим CPU.
                Thread.sleep(
                    when {
                        benchRecv -> 1000
                        currentRoom != null -> 2000
                        foreground -> 3000
                        else -> 15000
                    },
                )
            }
        }.apply {
            isDaemon = true
            name = "native-ticker"
            start()
        }
        // Active-speaker + трафик: пока в звонке — опрос ~4 раза/с (быстрее тикера).
        Thread {
            var prevBytes = emptyMap<String, Long>()
            var prevT = 0L
            while (running) {
                if (currentRoom != null && sessionPtr != 0L) {
                    runCatching {
                        val set = decodeList<String>(Native.roomSpeakingPeers(sessionPtr)).toSet()
                        if (speakingPeers != set) speakingPeers = set
                        // Трафик: скорость = дельта байт × 8 / dt.
                        val tj = decodeOrNull<Map<String, Long>>(Native.roomTrafficJson(sessionPtr)).orEmpty()
                        val now = System.currentTimeMillis()
                        val dt = ((now - prevT).coerceAtLeast(1)) / 1000.0
                        val cur = mutableMapOf<String, Long>()
                        val kbps = mutableMapOf<String, Int>()
                        tj.forEach { (k, b) ->
                            cur[k] = b
                            val pb = prevBytes[k] ?: b
                            kbps[k] = (((b - pb).coerceAtLeast(0)) * 8 / 1000 / dt).toInt()
                        }
                        prevBytes = cur
                        prevT = now
                        if (peerKbps != kbps) peerKbps = kbps
                    }
                    Thread.sleep(250)
                } else {
                    if (speakingPeers.isNotEmpty()) speakingPeers = emptySet()
                    if (peerKbps.isNotEmpty()) peerKbps = emptyMap()
                    prevBytes = emptyMap()
                    Thread.sleep(500)
                }
            }
        }.apply {
            isDaemon = true
            name = "active-speaker"
            start()
        }
    }

    // ── Контакты / чат ────────────────────────────────────────────────────────

    fun refreshContacts() = ops.execute { refreshContactsNow() }

    private fun refreshContactsNow() {
        val list = decodeList<ContactDto>(Native.listContacts(sessionPtr)).map { it.toModel() }
        contacts.clear()
        contacts.addAll(list)
        list.forEach { online.putIfAbsent(it.key, Native.isOnline(sessionPtr, it.key)) }
        // Аватары контактов (M2): байты уже подтянутого блоба по хэшу. Пусто → нет (ещё
        // не докачался или контакт без аватара) — покажем букву. Читаем на этом же [ops].
        list.forEach { c ->
            if (c.avatarId.isNotEmpty() && !contactAvatars.containsKey(c.avatarId)) {
                Native.blobRead(sessionPtr, c.avatarId).takeIf { it.isNotEmpty() }
                    ?.let { contactAvatars[c.avatarId] = it }
            }
        }
    }

    fun openChat(key: String) {
        currentChat = key
        unread[key] = 0
        ops.execute { loadHistory(key) }
    }

    private fun loadHistory(key: String) {
        val list = decodeList<MessageDto>(Native.history(sessionPtr, key))
            .map { it.toModel() }
            .toMutableList()
        // Ждущее досылки, которого НЕТ в базе, — добавляем обратно в ленту.
        //
        // Проверка «нет в базе» обязательна: при `Undelivered` сообщение остаётся в
        // истории движка И попадает в эту очередь, поэтому без фильтра каждое
        // недоставленное рисовалось дважды после перезахода в чат. Сверяем по тексту
        // и секунде отправки — id у очереди своего нет.
        synchronized(queueLock) { offlineQueueMap[key]?.toList() ?: emptyList() }
            .filterNot { (text, ts) -> list.any { it.mine && it.text == text && it.time == ts } }
            .forEach { (text, ts) -> list.add(Message(--localMsgId, text, mine = true, time = ts, status = "queued")) }
        messages[key] = list.toMutableStateList()
        atChatStart -= key // хвост перечитан — выше снова может что-то быть
        // M2: подтянуть байты вложений, которые уже докачаны (иначе — появятся по blob_received).
        list.forEach { if (it.blobId.isNotEmpty()) cacheBlob(it.blobId) }
    }

    private fun log(key: String): SnapshotStateList<Message> =
        messages.getOrPut(key) { mutableListOf<Message>().toMutableStateList() }

    /** Чаты, где мы уже дошли до начала переписки (просить выше нечего). */
    private val atChatStart = mutableSetOf<String>()

    /** Чаты/группы с идущей догрузкой — чтобы прокрутка не запускала её пачкой. */
    private val loadingOlder = mutableSetOf<String>()

    /**
     * Догрузить страницу истории ВЫШЕ показанной. Открытие чата отдаёт только хвост
     * (движок не поднимает переписку целиком), остальное приезжает по мере прокрутки.
     *
     * [done] получает, сколько сообщений добавлено — вызывающему это нужно, чтобы
     * удержать позицию списка: вставка сверху иначе выглядит как прыжок.
     */
    fun loadOlder(key: String, done: (Int) -> Unit) {
        val list = messages[key]
        // Курсор берём с первой строки, ПРИШЕДШЕЙ ИЗ БАЗЫ: у локальных (ожидающих
        // досылки) записей номера строки нет, ссылаться на них не на что.
        val top = list?.firstOrNull { it.row != 0L }
        if (list == null || top == null || key in atChatStart || !loadingOlder.add(key)) {
            done(0)
            return
        }
        ops.execute {
            try {
                val page = decodeList<MessageDto>(
                    Native.historyPage(sessionPtr, key, top.time, top.row),
                ).map { it.toModel() }
                if (page.isEmpty()) atChatStart += key
                if (page.isNotEmpty()) {
                    list.addAll(0, page)
                    page.forEach { if (it.blobId.isNotEmpty()) cacheBlob(it.blobId) }
                }
                done(page.size)
            } finally {
                loadingOlder -= key
            }
        }
    }

    /** То же для группы: путь общий, отличается только источник страницы. */
    fun loadOlderGroup(gid: String, done: (Int) -> Unit) {
        val list = groupMessages[gid]
        val top = list?.firstOrNull { it.row != 0L }
        val mark = "g:$gid"
        if (list == null || top == null || mark in atChatStart || !loadingOlder.add(mark)) {
            done(0)
            return
        }
        ops.execute {
            try {
                val page = decodeList<GroupMsgDto>(
                    Native.groupHistoryPage(sessionPtr, gid, top.time, top.row),
                ).map { it.toModel() }
                if (page.isEmpty()) atChatStart += mark
                if (page.isNotEmpty()) {
                    list.addAll(0, page)
                    page.forEach {
                        if (it.downloaded && it.blobId.isNotEmpty()) cacheBlob(it.blobId)
                        if (it.thumbId.isNotEmpty()) cacheBlob(it.thumbId)
                    }
                }
                done(page.size)
            } finally {
                loadingOlder -= mark
            }
        }
    }

    /**
     * Что переслать: источник и ссылки на сообщения. Непусто — открыт выбор адресата.
     * Выделение при этом уже снято: пока человек ищет чат, лента живёт своей жизнью.
     */
    var forwarding by mutableStateOf<Pair<ChatTarget, List<Pair<Long, String>>>?>(null)

    /** Начать пересылку выделенного в личном чате или «Сохранённом». */
    fun beginForward(chatKey: String) {
        val meKey = me?.pubkey.orEmpty()
        val refs = selection.map { it.id to (if (it.mine) meKey else chatKey) }
        if (refs.isEmpty()) return
        forwarding = ChatTarget(chatKey, group = false) to refs
        clearSelection()
    }

    /** Начать пересылку выделенного в группе (там автор известен явно). */
    fun beginForwardGroup(gid: String) {
        val refs = groupSelection.map { it.id to it.sender }
        if (refs.isEmpty()) return
        forwarding = ChatTarget(gid, group = true) to refs
        clearGroupSelection()
    }

    /**
     * Скрывать ли автора при пересылке. Решение принимается на КАЖДУЮ пересылку и
     * сбрасывается вместе с ней: «скрыл один раз» не должно молча распространяться
     * на все следующие.
     */
    var forwardHideAuthor by mutableStateOf(false)

    /** Отправить отложенное в выбранный чат. */
    fun forwardTo(dst: ChatTarget) {
        val (src, refs) = forwarding ?: return
        val hide = forwardHideAuthor
        forwarding = null
        forwardHideAuthor = false
        ops.execute {
            val json = WireJson.encodeToString(
                kotlinx.serialization.builtins.ListSerializer(GroupMsgRef.serializer()),
                refs.map { GroupMsgRef(it.first, it.second) },
            )
            Native.forwardMessages(sessionPtr, src.id, src.group, json, dst.id, dst.group, hide)
            // Целевой чат может быть открыт прямо сейчас — перечитываем его хвост.
            if (dst.group && currentGroup == dst.id) {
                loadGroupHistory(dst.id)
            } else if (!dst.group && currentChat == dst.id) {
                loadHistory(dst.id)
            } else if (!dst.group && dst.id == me?.pubkey) {
                refreshNotes()
            }
        }
    }

    /** hex автора цитируемого сообщения: «моё» — это я, иначе собеседник. */
    private fun replyAuthor(peerKey: String, reply: Message?): String = when {
        reply == null -> ""
        reply.mine -> me?.pubkey.orEmpty()
        else -> peerKey
    }

    fun send(key: String, text: String) {
        if (text.isBlank()) return
        // Цель ответа снимаем СРАЗУ: пользователь уже нажал «отправить», и оставленная
        // цитата в поле ввода означала бы, что следующее сообщение тоже уйдёт ответом.
        val reply = replyTarget
        replyTarget = null
        ops.execute {
            val now = System.currentTimeMillis() / 1000
            val id = Native.sendTextReply(
                sessionPtr,
                key,
                text,
                reply?.id ?: 0,
                reply?.mine ?: false,
            )
            if (id >= 0) {
                // "pending" — Rust подтверждение ещё не пришло; сменится на "delivered" по ACK.
                log(key).add(
                    Message(
                        id,
                        text,
                        mine = true,
                        time = now,
                        status = "pending",
                        replyTo = reply?.id ?: 0,
                        sender = me?.pubkey.orEmpty(),
                        replySender = replyAuthor(key, reply),
                    ),
                )
            } else {
                // Пир недостижим — кладём в очередь, доставим при появлении онлайн.
                val localId = --localMsgId
                log(key).add(
                    Message(
                        localId,
                        text,
                        mine = true,
                        time = now,
                        status = "queued",
                        replyTo = reply?.id ?: 0,
                        sender = me?.pubkey.orEmpty(),
                        replySender = replyAuthor(key, reply),
                    ),
                )
                enqueueOffline(key, text, now)
            }
        }
    }

    // ── Действия с сообщением (ADR-006) ───────────────────────────────────────

    /**
     * Что выделено в открытом чате. Ключ строки истории — ПАРА (id, mine): id посеян
     * временем отправителя, поэтому у моего и чужого сообщения он может совпасть.
     */
    var selection by mutableStateOf<Set<MsgRef>>(emptySet())
        private set

    /** На какое сообщение отвечаем (null — обычная отправка). */
    var replyTarget by mutableStateOf<Message?>(null)
        private set

    fun toggleSelect(m: Message) {
        val ref = MsgRef(m.id, m.mine)
        selection = if (ref in selection) selection - ref else selection + ref
    }

    fun clearSelection() {
        selection = emptySet()
    }

    fun startReply(m: Message) {
        replyTarget = m
        selection = emptySet()
    }

    fun cancelReply() {
        replyTarget = null
    }

    /** Текст выделенного для буфера обмена: несколько сообщений — с новой строки. */
    fun selectedText(key: String): String {
        val sel = selection
        return (messages[key] ?: emptyList<Message>())
            .filter { MsgRef(it.id, it.mine) in sel }
            .joinToString("\n") { if (it.text.isNotEmpty()) it.text else it.name }
    }

    /** Удалить выделенное только у себя. */
    fun deleteSelected(key: String) {
        val refs = selection.toList()
        if (refs.isEmpty()) return
        selection = emptySet()
        val json = WireJson.encodeToString(kotlinx.serialization.builtins.ListSerializer(MsgRef.serializer()), refs)
        ops.execute {
            Native.deleteMessages(sessionPtr, key, json)
            reloadHistory(key)
        }
    }

    /** Отозвать выделенные СВОИ сообщения у собеседника. Чужие не трогаем. */
    fun retractSelected(key: String) {
        val ids = selection.filter { it.mine }.map { it.id }
        if (ids.isEmpty()) return
        selection = emptySet()
        val json = ids.joinToString(prefix = "[", postfix = "]")
        ops.execute {
            Native.retractMessages(sessionPtr, key, json)
            reloadHistory(key)
        }
    }

    /** Удалить выделенные заметки (чат с самим собой). */
    fun deleteSelectedNotes() {
        val refs = selection.toList()
        if (refs.isEmpty()) return
        selection = emptySet()
        val key = me?.pubkey ?: return
        val json = WireJson.encodeToString(
            kotlinx.serialization.builtins.ListSerializer(MsgRef.serializer()),
            refs,
        )
        ops.execute {
            Native.deleteMessages(sessionPtr, key, json)
            val list = decodeList<MessageDto>(Native.notes(sessionPtr)).map { it.toModel() }
            notes.clear()
            notes.addAll(list)
        }
    }

    /** Перечитать историю чата после удаления/отзыва.
     *
     *  Именно [loadHistory], а не «очистить и долить»: он ЗАМЕНЯЕТ список целиком и
     *  возвращает в ленту неотправленное из офлайн-очереди (его в БД нет). Ручная
     *  чистка на месте оставляла в ленте старые копии — сообщения задваивались. */
    private fun reloadHistory(key: String) = loadHistory(key)

    /** Отправить вложение (M2): движок чистит картинку (scrub), шлёт блоб + медиа-
     *  сообщение. Хэш блоба считается от ОЧИЩЕННЫХ байт в движке, поэтому своё
     *  сообщение подтягиваем перечитыванием истории из стора (там верный blob_id). */
    fun sendMedia(key: String, content: ByteArray, mime: String, name: String) = ops.execute {
        if (Native.sendMedia(sessionPtr, key, content, mime, name) >= 0 && currentChat == key) {
            loadHistory(key)
        }
    }

    /** Отправить вложение с метаданными для карточки (длительность/кадр/превью).
     *  Их извлекает клиент — движок медиа не декодирует. */
    fun sendMediaWithMeta(
        key: String,
        content: ByteArray,
        mime: String,
        name: String,
        meta: MediaMeta,
        caption: String = "",
    ) = ops.execute {
        val id = Native.sendMediaWithMeta(
            sessionPtr, key, content, mime, name,
            meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
        )
        if (id >= 0 && currentChat == key) loadHistory(key)
    }

    /** Отправить вложение ФАЙЛОМ (потоково). Файл должен пережить отдачу — движок
     *  читает из него чанки по запросу получателя. */
    fun sendMediaFile(
        key: String,
        path: String,
        mime: String,
        name: String,
        meta: MediaMeta,
        caption: String = "",
    ) = ops.execute {
        val id = Native.sendMediaFile(
            sessionPtr, key, path, mime, name,
            meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
        )
        if (id >= 0 && currentChat == key) loadHistory(key)
    }

    // ── Флуд и смена адреса (ADR-007) ────────────────────────────────────────

    /** Кто сейчас заваливает нас трафиком (по убыванию темпа). Пусто — флуда нет. */
    val floodPeers = mutableStateListOf<FloodPeer>()

    /** Уведомление о текущем эпизоде флуда уже показано. */
    private var floodNotified = false

    /** Мы намеренно вне сети: кнопка «офлайн» или пауза смены адреса. */
    var offline by mutableStateOf(false)
        private set

    /** Эпоха адреса: 0 — адрес ни разу не менялся. */
    var transportEpoch by mutableStateOf(0)
        private set

    /** Ход смены адреса: null — не идёт. */
    var rotation by mutableStateOf<RotationState?>(null)
        private set

    /** Уйти в офлайн: разорвать соединения. Чужой поток перестаёт тратить трафик —
     *  ценой того, что и свои сообщения не приходят. */
    fun goOffline() = ops.execute {
        Native.goOffline(sessionPtr)
        offline = true
    }

    /** Вернуться в сеть на текущем адресе. */
    fun goOnline() = ops.execute {
        if (Native.goOnline(sessionPtr)) offline = false
    }

    /**
     * Сменить адрес: пауза офлайн [ROTATE_GAP_SECS] секунд, затем подъём на новом
     * ключе и рассылка подписанного объявления выбранным контактам.
     *
     * Пауза обязательна: без неё новый узел наследует маршрутную позицию старого
     * ключа и поток флудера продолжает приходить (проверено на публичном меше).
     * Поэтому её не прячем, а показываем обратным отсчётом.
     */
    fun rotateAddress(recipients: List<String>) {
        rotation = RotationState(secondsLeft = ROTATE_GAP_SECS, told = recipients.size)
        offline = true
        ops.execute { Native.rotateTransport(sessionPtr, recipients.joinToString(","), ROTATE_GAP_SECS) }
        // Отсчёт — отдельным потоком: сам движок в это время спит в паузе.
        Thread {
            for (left in ROTATE_GAP_SECS - 1 downTo 0) {
                Thread.sleep(1000)
                rotation = rotation?.copy(secondsLeft = left)
            }
        }.start()
    }

    /** Убрать итоговую карточку смены адреса. */
    fun dismissRotation() {
        rotation = null
    }

    /** Заказанные пользователем докачки: blobId → доля 0..1, [DOWNLOAD_INIT] — ждём
     *  первых чанков (пир может быть офлайн). Движок хранит намерение на диске и сам
     *  продолжает, когда пир появится, — следить за этим пользователь не должен. */
    val downloads = mutableStateMapOf<String, Float>()

    /** Подтянуть заказы, пережившие перезапуск (иначе карточка снова звала бы «Скачать»). */
    private fun loadWanted() {
        decodeList<String>(Native.wantedBlobs(sessionPtr)).forEach { id ->
            if (id.isNotEmpty() && !downloads.containsKey(id)) downloads[id] = DOWNLOAD_INIT
        }
    }

    /** Скачать вложение по кнопке (крупное не тянется автоматически).
     *  Готовность придёт событием `blob_received` — история перечитается. */
    fun downloadMedia(peer: String, blobId: String) {
        downloads[blobId] = DOWNLOAD_INIT
        ops.execute { Native.downloadMedia(sessionPtr, peer, blobId) }
    }

    /** Код для своего QR (ADR-007): личность + подписанный текущий адрес, если он
     *  менялся. Читается с движка, а не собирается в UI: формат — контракт движка. */
    fun myContactCode(): String = Native.myContactCode(sessionPtr)

    fun addContact(key: String, name: String) = ops.execute {
        val r = Native.addContact(sessionPtr, key, name)
        if (r == "added") refreshContactsNow()
        addStatus = r
    }

    fun toggleVerified(key: String) = ops.execute {
        val c = contacts.firstOrNull { it.key == key } ?: return@execute
        Native.setVerified(sessionPtr, key, !c.verified)
        refreshContactsNow()
    }

    fun rename(key: String, name: String) = ops.execute {
        Native.renameContact(sessionPtr, key, name)
        refreshContactsNow()
    }

    fun remove(key: String) = ops.execute {
        Native.removeContact(sessionPtr, key)
        if (currentChat == key) currentChat = null
        refreshContactsNow()
    }

    // ── Гейт контактов (запросы от незнакомцев) ────────────────────────────
    fun refreshRequests() = ops.execute { refreshRequestsNow() }

    private fun refreshRequestsNow() {
        val list = decodeList<ContactRequestDto>(Native.contactRequests(sessionPtr))
            .map { ContactRequest(it.key, it.fingerprint, it.preview, it.time) }
            .sortedByDescending { it.time }
        contactRequests.clear()
        contactRequests.addAll(list)
    }

    /** Одобрить запрос: незнакомец становится контактом. */
    fun acceptRequest(key: String) = ops.execute {
        if (Native.acceptContactRequest(sessionPtr, key)) {
            refreshRequestsNow()
            refreshContactsNow()
        }
    }

    /** Отклонить запрос (тихо). */
    fun declineRequest(key: String) = ops.execute {
        Native.declineContactRequest(sessionPtr, key)
        refreshRequestsNow()
    }

    /** Заблокировать пира: будущие сообщения тихо дропаются. */
    fun blockRequest(key: String) = ops.execute {
        Native.blockContact(sessionPtr, key)
        refreshRequestsNow()
        refreshContactsNow()
    }

    /** Применить одно событие из ядра. Вызывается из [pollThread] (фон). */
    private fun applyEvent(json: String) {
        if (json.isEmpty()) return
        // Разбор по ТИПУ, а не по строковым ключам: набор полей каждого события
        // зафиксирован в Dto.kt и сверяется компилятором с dto.rs.
        when (val ev = parseEvent(json)) {
            is EngineEvent.Presence -> {
                val wasOnline = online[ev.peer]
                online[ev.peer] = ev.online
                if (ev.online && wasOnline != true) flushQueueFor(ev.peer)
                if (contacts.none { it.key == ev.peer }) refreshContactsNow()
            }
            is EngineEvent.Text -> {
                if (contacts.none { it.key == ev.peer }) refreshContactsNow()
                log(ev.peer).add(
                    Message(
                        ev.id,
                        ev.text,
                        mine = false,
                        time = ev.time,
                        status = "",
                        replyTo = ev.replyTo,
                        sender = ev.peer,
                        // На проводе «чьё» едет флагом с точки зрения отправителя, и
                        // движок его уже перевернул: true здесь значит «моё».
                        replySender = if (ev.replyTo == 0L) {
                            ""
                        } else if (ev.replyMine) {
                            me?.pubkey.orEmpty()
                        } else {
                            ev.peer
                        },
                    ),
                )
                if (currentChat != ev.peer) unread[ev.peer] = (unread[ev.peer] ?: 0) + 1
            }
            // Собеседник отозвал своё сообщение: движок уже удалил его из истории,
            // здесь убираем из открытой ленты, чтобы не ждать перезахода в чат.
            is EngineEvent.Retracted -> {
                messages[ev.peer]?.removeAll { it.id == ev.id && !it.mine }
            }
            // M2: входящее медиа (ссылка на блоб; байты подтянутся блоб-каналом).
            is EngineEvent.Media -> {
                if (contacts.none { it.key == ev.peer }) refreshContactsNow()
                log(ev.peer).add(
                    Message(
                        id = ev.id, text = "", mine = false, time = ev.time, status = "",
                        kind = ev.kind, blobId = ev.blobId, mime = ev.mime, name = ev.name,
                        size = ev.size, durationMs = ev.durationMs,
                        width = ev.width, height = ev.height,
                        thumbId = ev.thumbId, downloaded = ev.downloaded,
                    ),
                )
                // Миниатюра приходит отдельным мелким блобом — подтянем, если уже есть.
                ev.thumbId.takeIf { it.isNotEmpty() }?.let { cacheBlob(it) }
                if (currentChat != ev.peer) unread[ev.peer] = (unread[ev.peer] ?: 0) + 1
            }
            is EngineEvent.Ack -> updateStatus(ev.peer, ev.id, "delivered")
            is EngineEvent.Undelivered -> {
                // Rust ретраил 5 минут и не смог — передаём в Kotlin-очередь.
                // Пир появится онлайн → flushQueueFor доставит автоматически.
                val l = messages[ev.peer] ?: return
                val idx = l.indexOfFirst { it.id == ev.id && it.mine }
                if (idx >= 0) {
                    val msg = l[idx]
                    l[idx] = msg.copy(status = "queued")
                    enqueueOffline(ev.peer, msg.text, msg.time)
                }
            }
            is EngineEvent.RoomInvite -> {
                // Стабильная комната: если мы УЖЕ в этой комнате с этим пиром, он
                // просто перезашёл — остаёмся в звонке, его возврат отразит presence.
                // НЕ рвём звонок и не показываем промпт заново (иначе teardown +
                // каскад decline — это и было «реентри ломает звонок»).
                if (currentRoom == ev.roomId && dmCallPeer == ev.peer) return
                val existing = incomingCall
                // Свободны — обычный входящий. Тот же собеседник перезванивает —
                // обновляем экран входящего. От другого пира, пока заняты — игнор.
                if (existing == null || existing.peer == ev.peer) {
                    incomingCall = IncomingRoomCall(ev.peer, ev.roomId, ev.audio, ev.video)
                    val callerName = contacts.firstOrNull { it.key == ev.peer }?.displayName
                        ?: appContext?.getString(R.string.common_contact).orEmpty()
                    appContext?.let { IncomingCallReceiver.show(it, callerName) }
                }
            }
            is EngineEvent.RoomDecline -> {
                if (ev.peer == dmCallPeer) leaveRoom()
                // Звонящий сбросил до того как мы ответили — гасим уведомление.
                if (incomingCall?.peer == ev.peer) {
                    incomingCall = null
                    appContext?.let { IncomingCallReceiver.cancel(it) }
                }
            }
            is EngineEvent.RoomPresence -> if (ev.gid == currentRoom) {
                // Мгновенно отражаем участника, не дожидаясь опроса тикером.
                if (ev.left) {
                    roomMembers.removeAll { it.peer == ev.peer } // явный выход — убрать сразу
                } else {
                    val rm = RoomMember(ev.peer, ev.flags and 0b01 != 0, ev.flags and 0b10 != 0)
                    val idx = roomMembers.indexOfFirst { it.peer == ev.peer }
                    if (idx >= 0) roomMembers[idx] = rm else roomMembers.add(rm)
                }
            }
            // Гейт: незнакомец постучался — обновить очередь запросов (карточка в UI).
            is EngineEvent.ContactRequestEv -> refreshRequestsNow()
            // Приглашение в группу пришло/принято/отклонено — обновить карточки.
            EngineEvent.GroupInvitesChanged -> refreshGroupInvitesNow()
            // M2: контакт прислал профиль (имя/аватар) → перечитать список.
            EngineEvent.ContactsChanged -> refreshContactsNow()
            // Адрес сменился: обновляем показ и завершаем карточку хода.
            is EngineEvent.TransportRotated -> {
                transportEpoch = ev.epoch
                offline = false
                rotation = rotation?.copy(secondsLeft = 0, newAddress = ev.address)
                me = me?.copy(address = ev.address)
            }
            is EngineEvent.BlobProgress -> {
                if (ev.total > 0) downloads[ev.id] = ev.done / ev.total.toFloat()
            }
            // Докачался блоб: аватар контакта — перечитать контакты; вложение чата —
            // положить байты в реактивный кэш (пузырь перерисуется).
            is EngineEvent.BlobReceived -> {
                downloads.remove(ev.id)
                if (contacts.any { it.avatarId == ev.id }) refreshContactsNow()
                cacheBlob(ev.id)
            }
            EngineEvent.GroupsChanged -> {
                refreshGroupsNow()
                currentGroup?.let {
                    loadGroupHistory(it)
                    loadGroupMembers(it)
                }
            }
            is EngineEvent.GroupRemoved -> {
                // ADR-004: нас отозвали. Если открыт этот чат — выходим и уведомляем.
                if (currentGroup == ev.gid) currentGroup = null
                appContext?.let { ctx ->
                    android.os.Handler(android.os.Looper.getMainLooper()).post {
                        android.widget.Toast.makeText(
                            ctx,
                            ctx.getString(R.string.group_you_removed),
                            android.widget.Toast.LENGTH_LONG,
                        ).show()
                    }
                }
            }
            is EngineEvent.GroupMsgEv -> {
                if (ev.mine) {
                    // Своё групповое медиа: blob_id считается от очищенных байт в
                    // движке — перечитываем историю, чтобы показать верный хэш.
                    if (currentGroup == ev.gid) loadGroupHistory(ev.gid)
                } else if (currentGroup == ev.gid) {
                    // Чат открыт — перечитываем историю целиком: id и ссылку ответа
                    // знает движок, а событие их не несёт. Без этого свежее сообщение
                    // нельзя было бы ни выделить, ни процитировать до перезахода.
                    val md = ev.media
                    if (md != null && md.downloaded) cacheBlob(md.blobId)
                    md?.thumbId?.takeIf { it.isNotEmpty() }?.let { cacheBlob(it) }
                    loadGroupHistory(ev.gid)
                } else {
                    val md = ev.media
                    // Вложение: байты берём из кэша, только если оно уже скачано —
                    // крупное ждёт кнопки, и тянуть его «на всякий случай» нельзя.
                    if (md != null && md.downloaded) cacheBlob(md.blobId)
                    md?.thumbId?.takeIf { it.isNotEmpty() }?.let { cacheBlob(it) }
                    groupLog(ev.gid).add(
                        GroupMsg(
                            text = ev.text,
                            mine = false,
                            time = System.currentTimeMillis() / 1000,
                            sender = ev.sender,
                            system = ev.system,
                            kind = md?.kind ?: "text",
                            blobId = md?.blobId.orEmpty(),
                            mime = md?.mime.orEmpty(),
                            name = md?.name.orEmpty(),
                            size = md?.size ?: 0,
                            durationMs = md?.durationMs ?: 0,
                            width = md?.width ?: 0,
                            height = md?.height ?: 0,
                            thumbId = md?.thumbId.orEmpty(),
                            downloaded = md?.downloaded ?: false,
                        ),
                    )
                }
            }
            // В группе отозвали сообщение — перечитываем ленту целиком.
            is EngineEvent.GroupRetracted -> loadGroupHistory(ev.gid)
            // Событие, которого этот клиент ещё не знает: молча пропускаем.
            EngineEvent.Unknown -> Unit
        }
    }

    private fun updateStatus(peer: String, id: Long, status: String) {
        val l = messages[peer] ?: return
        val idx = l.indexOfFirst { it.id == id && it.mine }
        if (idx >= 0) l[idx] = l[idx].copy(status = status)
    }

    // ── Офлайн-очередь ────────────────────────────────────────────────────────

    private fun loadOfflineQueue() {
        val f = queueFile ?: return
        if (!f.exists()) return
        try {
            val json = JSONObject(f.readText())
            synchronized(queueLock) {
                json.keys().forEach { peer ->
                    val arr = json.getJSONArray(peer)
                    // Срок хранения НЕ ограничиваем: сервера, который сохранит письмо
                    // за нас, нет — собеседник может вернуться через месяц и обязан
                    // его получить. Объём держит бюджет пира в движке (Tier.outbox_cap).
                    val msgs = (0 until arr.length()).map { i ->
                        val o = arr.getJSONObject(i)
                        Pair(o.getString("text"), o.getLong("ts"))
                    }
                    if (msgs.isNotEmpty()) offlineQueueMap[peer] = msgs.toMutableList()
                }
            }
        } catch (e: Exception) {
            android.util.Log.w("p2pqueue", "load failed: $e")
        }
    }

    private fun saveOfflineQueue() {
        try {
            val json = JSONObject()
            synchronized(queueLock) {
                offlineQueueMap.forEach { (peer, msgs) ->
                    if (msgs.isNotEmpty()) {
                        val arr = JSONArray()
                        msgs.forEach { (text, ts) -> arr.put(JSONObject().put("text", text).put("ts", ts)) }
                        json.put(peer, arr)
                    }
                }
            }
            queueFile?.writeText(json.toString())
        } catch (e: Exception) {
            android.util.Log.w("p2pqueue", "save failed: $e")
        }
    }

    private fun enqueueOffline(peer: String, text: String, ts: Long) {
        synchronized(queueLock) {
            offlineQueueMap.getOrPut(peer) { mutableListOf() }.add(Pair(text, ts))
        }
        saveOfflineQueue()
    }

    /** Восстановить очередь в лог при старте (пользователь видит «ожидание» после перезапуска). */
    private fun restoreQueuedToLog() {
        synchronized(queueLock) {
            offlineQueueMap.forEach { (peer, msgs) ->
                val l = log(peer)
                msgs.forEach { (text, ts) ->
                    l.add(Message(--localMsgId, text, mine = true, time = ts, status = "queued"))
                }
            }
        }
    }

    /** Сброс очереди для пира, который появился онлайн.
     *  Всё выполняется на ops-потоке, чтобы не гонять с concurrent send(). */
    private fun flushQueueFor(peer: String) {
        ops.execute {
            // remove внутри ops: сериализован с send(), нет гонки «новое сообщение→flush удаляет его из лога».
            val msgs = synchronized(queueLock) {
                val q = offlineQueueMap.remove(peer) ?: return@execute
                q.toList()
            }
            saveOfflineQueue()
            // Убираем «ожидание»-заглушки из лога — они вернутся с настоящими ID.
            messages[peer]?.removeAll { it.mine && it.status == "queued" }
            msgs.forEach { (text, _) ->
                val now = System.currentTimeMillis() / 1000
                val id = Native.sendText(sessionPtr, peer, text)
                if (id >= 0) {
                    log(peer).add(Message(id, text, mine = true, time = now, status = "pending"))
                } else {
                    // Всё равно не достижим — вернём в очередь.
                    enqueueOffline(peer, text, now)
                }
            }
        }
    }

    // ── DM-звонки (через комнату) ─────────────────────────────────────────────

    /** Позвонить пиру: создать DM-комнату, отправить приглашение, начать медиа. */
    fun callDm(peer: String, audio: Boolean = true, video: Boolean = false) {
        if (currentRoom != null) return
        val ctx = appContext ?: return
        val name = contacts.firstOrNull { it.key == peer }?.displayName ?: ""
        ops.execute {
            val roomId = Native.roomInviteDm(sessionPtr, peer, audio, video)
            if (roomId.isEmpty()) return@execute
            currentRoom = roomId
            dmCallPeer = peer
            roomMembers.clear()
            CallService.start(ctx, name)
            startRoomMedia(ctx, roomId, video)
        }
    }

    /** Принять входящий DM-звонок с ПРЕСЕТОМ своего mic/video (выбор join'ера до
     *  входа, НЕ флаги звонящего). Дефолт — мик вкл, видео выкл. */
    fun acceptDmCall(audio: Boolean = true, video: Boolean = false) {
        val ic = incomingCall ?: return
        incomingCall = null
        val ctx = appContext ?: return
        IncomingCallReceiver.cancel(ctx)
        val name = contacts.firstOrNull { it.key == ic.peer }?.displayName ?: ""
        muted = !audio // мик-пресет → стартовое состояние мьюта
        ops.execute {
            val roomId = Native.roomJoinDm(sessionPtr, ic.peer, audio, video)
            if (roomId.isEmpty()) return@execute
            currentRoom = roomId
            dmCallPeer = ic.peer
            roomMembers.clear()
            CallService.start(ctx, name)
            startRoomMedia(ctx, roomId, video)
        }
    }

    /** Отклонить входящий DM-звонок. */
    fun declineDmCall() {
        val ic = incomingCall ?: return
        incomingCall = null
        appContext?.let { IncomingCallReceiver.cancel(it) }
        ops.execute { Native.roomDeclineDm(sessionPtr, ic.peer) }
    }

    /** Создать аудио-сессию комнаты (Opus-битрейт/режим + fx + устройство из настроек).
     *  Используется при старте звонка и при «Применить» в настройках звука (рестарт). */
    private fun startRoomAudioSession(ctx: android.content.Context, gid: String) {
        // битрейт/режим из настроек звука (audioBitrate: дефолт 24к=голос при «Авто»);
        // music = Opus Audio-режим на верхней половине диапазона.
        val params = decodeOrNull<CallParamsDto>(
            Native.roomAudioStart(sessionPtr, gid, audioBitrate, audioBitrate >= 32_000),
        )
        val sr = params?.sampleRate?.takeIf { it > 0 } ?: 48_000
        val ch = params?.channels?.takeIf { it > 0 } ?: 1
        roomAudio = CallAudio(
            ctx,
            { data, n -> Native.roomPushAudio(sessionPtr, data, n) },
            { out -> Native.roomPullAudio(sessionPtr, out) },
            { m -> Native.roomSetMuted(sessionPtr, m) },
            fx = audioFx,
            inputDevice = selectedInputDevice(),
            onLevel = { lvl -> if (showAudioSettings) micLevel = lvl }, // только когда экран открыт
        ).also {
            it.start(sr, ch)
            it.setMuted(muted)
        } // mic-пресет применяем сразу
        applyOutputDevice() // применить выбранный вывод (встроенный/динамик/bt) к свежей сессии
    }

    /** Применить настройки звука НА ЛЕТУ — без перезвона и без дыры в звуке:
     *  вывод/вход/AEC-NS-AGC/битрейт меняются на работающей сессии. Режим Opus
     *  (Голос↔Аудио) — только на следующем звонке (live менять нельзя). */
    fun applyAudioSettings() {
        showAudioSettings = false
        applyOutputDevice() // вывод — через AudioManager
        roomAudio?.applyFx(audioFx) // обработка — на существующих эффектах
        roomAudio?.setInputDevice(selectedInputDevice()) // вход — preferredDevice
        if (currentRoom != null) {
            ops.execute { Native.roomSetAudioBitrate(sessionPtr, audioBitrate) }
        }
    }

    private fun startRoomMedia(ctx: android.content.Context, gid: String, video: Boolean) {
        startRoomAudioSession(ctx, gid)
        if (video) {
            videoActive = true
            maybeStartVideoSender() // стартует, когда surface тайла «Вы» будет готов
        }
    }

    /** Подключить surface тайла к видео-потоку участника. Идемпотентно: если у пира
     *  уже есть приёмник — не пересоздаём (защита от тёрки декодеров). */
    fun attachVideoReceiver(peerHex: String, surface: android.view.Surface) {
        if (videoReceivers.containsKey(peerHex)) return
        videoReceivers[peerHex] = VideoReceiver(
            surface,
            videoQuality.width,
            videoQuality.height,
            videoQuality.fps,
            onRotation = { rot -> roomMemberRotation[peerHex] = rot },
        ) { Native.roomPullVideoPacket(sessionPtr, peerHex) }.also { it.start() }
    }

    /** Отключить приёмник пира (его тайл ушёл — вышел / перестал слать видео). */
    fun detachVideoReceiver(peerHex: String) {
        videoReceivers.remove(peerHex)?.stop()
    }

    /** Переключить заглушение микрофона (кнопка в экране звонка). */
    fun toggleMute() {
        muted = !muted
        roomAudio?.setMuted(muted)
        propagateAvFlags()
    }

    /** Вкл/выкл свою камеру по ходу звонка (кнопка «Видео»). */
    fun toggleVideo() {
        if (videoActive) {
            videoActive = false
            stopVideoSender()
        } else {
            videoActive = true
            maybeStartVideoSender() // реально стартует, когда surface тайла «Вы» готов
        }
        propagateAvFlags()
    }

    /** Разослать текущие A/V-флаги (маячок), чтобы собеседники знали, что у нас
     *  сейчас включено, и подписались/отписались на наше видео. roomJoin обновляет
     *  флаги комнаты + шлёт presence (для DM — пиру). */
    private fun propagateAvFlags() {
        val gid = currentRoom ?: return
        val audioOn = !muted
        val videoOn = videoActive
        ops.execute { Native.roomJoin(sessionPtr, gid, audioOn, videoOn) }
    }

    /** Стартуем отправку видео ТОЛЬКО когда совпало: видео включено, surface тайла
     *  «Вы» готов (он же — превью), отправитель ещё не запущен. Камера рендерит в
     *  энкодер + превью ОДНОЙ сессией (без хрупкой пересборки). Нет surface (ты
     *  один) → не кодируем/не шлём — ровно как в дизайне. */
    private fun maybeStartVideoSender() {
        val ctx = appContext ?: return
        val preview = selfPreviewSurface
        if (!videoActive || videoSender != null || preview == null) return
        val cap = when {
            videoQuality.height >= 1080 -> 24
            videoQuality.height >= 720 -> 14
            else -> 9
        }
        Native.roomSetVideoCap(sessionPtr, cap)
        // Стартуем с текущей стороны камеры (cameraFront): дефолт — фронталка, но
        // экран шаринга мог выбрать тыловую (applyVideoShare ставит cameraFront).
        videoSender = VideoSender(
            ctx,
            videoQuality.width,
            videoQuality.height,
            videoQuality.fps,
            videoQuality.bitrate,
            startFront = cameraFront,
        ) { data, key -> Native.roomPushVideo(sessionPtr, data, key) }.also { it.start(preview) }
    }

    private fun stopVideoSender() {
        val vs = videoSender
        videoSender = null
        Thread { runCatching { vs?.stop() } }.apply { isDaemon = true }.start()
    }

    /** Переключить фронт/тыл по ходу звонка (чип смены камеры). */
    fun switchCamera() {
        cameraFront = !cameraFront
        videoSender?.switchCamera()
    }

    /** Surface тайла «Вы» появился/ушёл. Появился → можно стартовать отправку;
     *  ушёл при включённом видео (ты остался один, тайла «Вы» нет) → стоп отправки. */
    fun setSelfPreviewSurface(surface: android.view.Surface?) {
        selfPreviewSurface = surface
        if (surface != null) {
            maybeStartVideoSender()
        } else if (videoActive) {
            stopVideoSender()
        }
    }

    // ── Комната-звонок (присутствие, Discord-style) ───────────────────────────

    /** Войти в комнату группы: маячок присутствия + мультипати аудио (микрофон
     *  всем участникам, приём от каждого с миксом). */
    fun joinRoom(gid: String, audio: Boolean = true, video: Boolean = false) {
        currentRoom = gid
        dmCallPeer = null
        roomMembers.clear()
        val ctx = appContext ?: return
        val name = groups.firstOrNull { it.gid == gid }?.name ?: ""
        ops.execute {
            Native.roomJoin(sessionPtr, gid, audio, video)
            CallService.start(ctx, name)
            startRoomMedia(ctx, gid, video)
        }
    }

    /** Выйти из комнаты: стоп медиа + маячок «вышел». */
    fun leaveRoom() {
        val gid = currentRoom ?: return
        // Запомнить ДО очистки. peerPresent: активный звонок (пир в комнате) vs
        // ещё дозваниваемся (пира в комнате нет) — от этого зависит, слать ли decline.
        val peer = dmCallPeer
        val peerPresent = peer != null && roomMembers.any { it.peer == peer }
        currentRoom = null
        dmCallPeer = null
        videoActive = false
        roomMembers.clear()
        muted = false
        micLevel = 0f // вывод (audioOut) теперь персистится как настройка — не сбрасываем
        val a = roomAudio
        roomAudio = null
        val vs = videoSender
        videoSender = null
        val vrs = videoReceivers.values.toList()
        videoReceivers.clear()
        val ctx = appContext
        ops.execute {
            // RoomDecline шлём ТОЛЬКО чтобы отменить ДОЗВОН (пир ещё не вошёл) → у
            // него гаснет экран входящего+уведомление. Выход из АКТИВНОЙ комнаты —
            // только presence LEFT (roomLeave): пир ОСТАЁТСЯ в комнате (комнатная
            // модель), мы не «кладём ему трубку».
            if (peer != null && !peerPresent) Native.roomDeclineDm(sessionPtr, peer)
            Native.roomLeave(sessionPtr, gid)
            Native.roomAudioStop(sessionPtr)
            if (ctx != null) CallService.stop(ctx)
        }
        Thread {
            runCatching { a?.stop() }
            runCatching { vs?.stop() }
            vrs.forEach { runCatching { it.stop() } }
        }.apply { isDaemon = true }.start()
    }

    // ── Группы ──────────────────────────────────────────────────────────────

    fun refreshGroups() = ops.execute { refreshGroupsNow() }

    private fun refreshGroupsNow() {
        val list = decodeList<GroupRowDto>(Native.groupList(sessionPtr))
            .map { Group(it.gid, it.name, it.members, it.admin, it.archived) }
        groups.clear()
        groups.addAll(list)
    }

    // ── Приглашения в группы (карточки на одобрение) ───────────────────────
    fun refreshGroupInvites() = ops.execute { refreshGroupInvitesNow() }

    private fun refreshGroupInvitesNow() {
        val list = decodeList<GroupInviteDto>(Native.groupInvites(sessionPtr))
            .map { GroupInvite(it.gid, it.name, it.inviter, it.waiting) }
        groupInvites.clear()
        groupInvites.addAll(list)
    }

    /** Принять приглашение: вступление завершится по `groups_changed`. */
    fun acceptGroupInvite(gid: String) = ops.execute {
        Native.acceptGroupInvite(sessionPtr, gid)
        // Состояние карточки («ждём ответа») выставит движок — команду исполняет
        // актор в своём потоке, и читать сразу после вызова рано.
    }

    /** Отклонить приглашение (тихо). */
    fun declineGroupInvite(gid: String) = ops.execute {
        Native.declineGroupInvite(sessionPtr, gid)
        refreshGroupInvitesNow()
    }

    /** Ключи участников группы (hex, кроме себя) — для экрана управления. */
    val groupMemberKeys = mutableStateMapOf<String, List<String>>()

    /** Мы ли админ группы (гейт кнопки «удалить»). */
    fun isGroupAdmin(gid: String): Boolean = groups.firstOrNull { it.gid == gid }?.admin == true

    /** Имя для показа по hex-ключу: имя контакта (local→profile→hex) либо укороченный ключ. */
    fun peerName(hex: String): String =
        contacts.firstOrNull { it.key == hex }?.displayName ?: (hex.take(8) + "…")

    fun loadGroupMembers(gid: String) = ops.execute {
        groupMemberKeys[gid] = decodeList<String>(Native.groupMembers(sessionPtr, gid))
    }

    /** Отозвать участника (ADR-004, только админ). Ростер обновит `groups_changed`. */
    fun removeGroupMember(gid: String, peerHex: String) = ops.execute {
        Native.groupRemoveMember(sessionPtr, gid, peerHex)
        loadGroupMembers(gid)
    }

    /** Выйти из группы (ADR-004). Локально забываем сразу; движок просит админа отозвать. */
    fun leaveGroup(gid: String) {
        if (currentGroup == gid) currentGroup = null
        ops.execute { Native.groupLeave(sessionPtr, gid) }
    }

    /** Создать группу (fire-and-forget). Появится в списке по событию
     *  `groups_changed` (см. applyEvent → refreshGroupsNow). */
    fun createGroup(name: String) = ops.execute {
        Native.groupCreate(sessionPtr, name.ifBlank { appContext?.getString(R.string.group_default_name) ?: "Group" })
    }

    fun openGroup(gid: String) {
        currentGroup = gid
        currentGroupRoster = null // тикер подтянет свежий агрегат
        loadGroupMembers(gid)
        ops.execute { loadGroupHistory(gid) }
    }

    private fun loadGroupHistory(gid: String) {
        // Флаг `downloaded` из истории уточняем фактом наличия блоба: своё вложение
        // и всё скачанное до появления флага иначе показывалось бы карточкой
        // «скачать». Проверка не читает байты — только наличие файла.
        val list = decodeList<GroupMsgDto>(Native.groupHistory(sessionPtr, gid)).map { it.toModel() }
            .map {
                if (!it.downloaded && it.blobId.isNotEmpty() && Native.blobExists(sessionPtr, it.blobId)) {
                    it.copy(downloaded = true)
                } else {
                    it
                }
            }
        groupMessages[gid] = list.toMutableStateList()
        atChatStart -= "g:$gid"
        // M2: подтянуть байты уже скачанных вложений (иначе — по blob_received);
        // миниатюры — всегда, на них держится карточка ещё не скачанного.
        list.forEach {
            if (it.downloaded && it.blobId.isNotEmpty()) cacheBlob(it.blobId)
            if (it.thumbId.isNotEmpty()) cacheBlob(it.thumbId)
        }
    }

    private fun groupLog(gid: String): SnapshotStateList<GroupMsg> =
        groupMessages.getOrPut(gid) { mutableListOf<GroupMsg>().toMutableStateList() }

    fun inviteToGroup(gid: String, peerHex: String) = ops.execute { Native.groupInvite(sessionPtr, gid, peerHex) }

    fun sendGroupText(gid: String, text: String) {
        if (text.isBlank()) return
        val reply = groupReplyTarget
        groupReplyTarget = null
        ops.execute {
            Native.groupSendTextReply(
                sessionPtr,
                gid,
                text,
                reply?.id ?: 0,
                reply?.sender.orEmpty(),
            )
            // Ленту перечитает обработчик события от движка: команда исполняется в
            // потоке-акторе, и сразу после возврата из JNI история ещё старая.
        }
    }

    // ── Действия с групповым сообщением (ADR-006) ────────────────────────────

    var groupSelection by mutableStateOf<Set<GroupMsgRef>>(emptySet())
        private set

    var groupReplyTarget by mutableStateOf<GroupMsg?>(null)
        private set

    fun toggleGroupSelect(m: GroupMsg) {
        // Сообщения без id (старый клиент) выбрать нельзя: с ними нечего делать —
        // ни отозвать, ни ответить, а «удалить у себя» без ссылки не адресуется.
        if (m.id == 0L) return
        val ref = GroupMsgRef(m.id, m.sender)
        groupSelection = if (ref in groupSelection) groupSelection - ref else groupSelection + ref
    }

    fun clearGroupSelection() {
        groupSelection = emptySet()
    }

    fun startGroupReply(m: GroupMsg) {
        groupReplyTarget = m
        groupSelection = emptySet()
    }

    fun cancelGroupReply() {
        groupReplyTarget = null
    }

    fun groupSelectedText(gid: String): String {
        val sel = groupSelection
        return (groupMessages[gid] ?: emptyList<GroupMsg>())
            .filter { GroupMsgRef(it.id, it.sender) in sel }
            .joinToString("\n") { if (it.text.isNotEmpty()) it.text else it.name }
    }

    fun deleteGroupSelected(gid: String) {
        val refs = groupSelection.toList()
        if (refs.isEmpty()) return
        groupSelection = emptySet()
        val json = WireJson.encodeToString(
            kotlinx.serialization.builtins.ListSerializer(GroupMsgRef.serializer()),
            refs,
        )
        ops.execute {
            Native.groupDeleteMessages(sessionPtr, gid, json)
        }
    }

    /** Отозвать выделенные СВОИ сообщения у участников. Чужие не трогаем. */
    fun retractGroupSelected(gid: String) {
        val mine = me?.pubkey.orEmpty()
        val ids = groupSelection.filter { it.sender == mine }.map { it.id }
        if (ids.isEmpty()) return
        groupSelection = emptySet()
        val json = ids.joinToString(prefix = "[", postfix = "]")
        ops.execute {
            Native.groupRetractMessages(sessionPtr, gid, json)
        }
    }

    /** Отправить вложение в группу (M2): движок чистит картинку, шлёт блоб всем
     *  участникам + MLS-ссылку. Своё сообщение подтянется перечитыванием истории
     *  (event group_msg mine=true), т.к. blob_id считается от очищенных байт в движке. */
    fun sendGroupMedia(
        gid: String,
        content: ByteArray,
        mime: String,
        name: String,
        meta: MediaMeta = MediaMeta(),
        caption: String = "",
    ) = ops.execute {
        Native.groupSendMedia(
            sessionPtr, gid, content, mime, name,
            meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
        )
    }

    /** Отправить в группу вложение ФАЙЛОМ (потоково). Файл должен пережить отдачу:
     *  движок читает из него чанки по запросу участников. */
    fun sendGroupMediaFile(
        gid: String,
        path: String,
        mime: String,
        name: String,
        meta: MediaMeta = MediaMeta(),
        caption: String = "",
    ) = ops.execute {
        Native.groupSendMediaFile(
            sessionPtr, gid, path, mime, name,
            meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
        )
    }

    // ── Диагностика: тест пропускной способности ─────────────────────────────

    /** Включить режим приёмника бенча (живая скорость в [benchRecvMbps]). */
    fun startBenchRecv() {
        benchRecv = true
        ops.execute { Native.benchRecvStart(sessionPtr) }
    }

    /** Блб ~30 МБ на пира; результат отправки в [benchResult]. */
    fun blastBench(peerHex: String) {
        benchResult = appContext?.getString(R.string.bench_sending).orEmpty()
        ops.execute {
            val r = decodeOrNull<BenchDto>(Native.benchBlast(sessionPtr, peerHex, 30_000))
            benchResult = appContext?.getString(
                R.string.bench_sent,
                r?.mbps ?: 0.0,
                (r?.sentBytes ?: 0L) / 1e6,
            ).orEmpty()
        }
    }
}
