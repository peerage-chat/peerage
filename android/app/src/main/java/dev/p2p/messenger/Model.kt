package dev.p2p.messenger

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.toMutableStateList
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors

/** Параметры видео-звонка (аппаратный H.264 через MediaCodec). */
const val VIDEO_W = 1280
const val VIDEO_H = 720
const val VIDEO_FPS = 30
const val VIDEO_BITRATE = 2_000_000 // 2 Мбит/с — хорошее качество для 720p30

data class Contact(
    val key: String,
    val name: String,
    val fingerprint: String,
    val verified: Boolean,
)

data class Message(
    val id: Long,
    val text: String,
    val mine: Boolean,
    val time: Long,
    val status: String,
)

/** Идентичность пользователя (своя «визитка»). */
data class Me(val pubkey: String, val address: String, val fingerprint: String)

data class Group(val gid: String, val name: String, val members: Int)
data class GroupMsg(val text: String, val mine: Boolean, val time: Long)

/** Состояние звонка. status: "incoming" | "ringing" | "active". */
data class CallState(
    val peer: String,
    val callId: Long,
    val incoming: Boolean,
    val withVideo: Boolean,
    val status: String,
    val muted: Boolean = false,
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
        /** Единственный экземпляр на процесс. Нативный узел/потоки тоже одни на
         *  процесс, поэтому модель должна переживать пересоздание Activity (иначе
         *  утекают второй узел и дублирующие poll/ticker-циклы → потеря событий). */
        val instance: AppModel by lazy { AppModel() }
    }

    var me by mutableStateOf<Me?>(null)
    var error by mutableStateOf<String?>(null)
    var sessionPtr: Long = 0L

    val contacts = mutableStateListOf<Contact>()
    val online = mutableStateMapOf<String, Boolean>()
    val unread = mutableStateMapOf<String, Int>()
    val messages = mutableStateMapOf<String, SnapshotStateList<Message>>()

    var currentChat by mutableStateOf<String?>(null)
    var netPeers by mutableStateOf(0)
    /** Результат последней попытки добавить контакт ("" | added | known | error). */
    var addStatus by mutableStateOf("")

    // Диагностика: тест пропускной способности меша.
    var benchRecv by mutableStateOf(false)   // режим приёмника включён
    var benchRecvMbps by mutableStateOf(0.0) // живая скорость приёма
    var benchResult by mutableStateOf("")    // результат отправки (для blast)

    // Группы (MLS).
    val groups = mutableStateListOf<Group>()
    val groupMessages = mutableStateMapOf<String, SnapshotStateList<GroupMsg>>()
    var currentGroup by mutableStateOf<String?>(null)

    // Комната-звонок (Discord-style присутствие). gid комнаты, где мы сейчас.
    var currentRoom by mutableStateOf<String?>(null)
    val roomMembers = mutableStateListOf<RoomMember>()

    // Звонок (1:1). audio/appContext задаются из Activity (нужен Context).
    var call by mutableStateOf<CallState?>(null)
    var audio: CallAudio? = null
    var appContext: android.content.Context? = null
    @Volatile private var videoSender: VideoSender? = null
    @Volatile private var roomAudio: CallAudio? = null

    // Фоновое взаимодействие с ядром (никогда не на главном потоке).
    private val ops = Executors.newSingleThreadExecutor { Thread(it, "native-ops").apply { isDaemon = true } }
    @Volatile private var running = false
    private var pollThread: Thread? = null
    private var tickerThread: Thread? = null

    @Volatile private var started = false

    /** Инициализация ядра. Звать с фонового потока (см. MainActivity).
     *  Идемпотентно: при пересоздании Activity повторно НЕ инициализирует. */
    fun start(dataDir: String) {
        if (started) return
        started = true
        val json = JSONObject(Native.start(dataDir))
        if (json.has("error")) {
            error = json.getString("error")
            return
        }
        sessionPtr = json.getLong("session_ptr")
        me = Me(
            pubkey = json.getString("pubkey"),
            address = json.getString("address"),
            fingerprint = json.getString("fingerprint"),
        )
        refreshContactsNow()
        refreshGroupsNow()
        startLoops()
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
                Thread.sleep(80)
            }
        }.apply { isDaemon = true; name = "native-poll"; start() }
        tickerThread = Thread {
            var tick = 0L
            while (running) {
                runCatching {
                    val p = Native.netPeers(sessionPtr)
                    val t = Native.netTree(sessionPtr)
                    netPeers = p
                    contacts.map { it.key }.forEach { online[it] = Native.isOnline(sessionPtr, it) }
                    if (benchRecv) {
                        benchRecvMbps = JSONObject(Native.benchRecvStats(sessionPtr)).optDouble("mbps", 0.0)
                    }
                    // Присутствие в комнате: кто сейчас внутри (по свежим маячкам).
                    currentRoom?.let { gid ->
                        val arr = JSONArray(Native.roomParticipants(sessionPtr, gid))
                        val list = (0 until arr.length()).map {
                            val o = arr.getJSONObject(it)
                            RoomMember(o.getString("peer"), o.optBoolean("audio"), o.optBoolean("video"))
                        }
                        roomMembers.clear(); roomMembers.addAll(list)
                    }
                    // Диагностика устойчивости сети: heartbeat + пиры/дерево. Если
                    // tick перестаёт расти при погасшем экране — потоки заморожены;
                    // если peers=0 и не возвращается — отвал пиринга (не reconnect).
                    android.util.Log.i("p2pnet", "tick=$tick peers=$p tree=$t inCall=${call != null} room=${currentRoom != null}")
                }
                tick++
                Thread.sleep(if (benchRecv) 1000 else if (currentRoom != null) 2000 else 3000)
            }
        }.apply { isDaemon = true; name = "native-ticker"; start() }
    }

    // ── Контакты / чат ────────────────────────────────────────────────────────

    fun refreshContacts() = ops.execute { refreshContactsNow() }

    private fun refreshContactsNow() {
        val arr = JSONArray(Native.listContacts(sessionPtr))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Contact(o.getString("key"), o.getString("name"), o.optString("fingerprint"), o.optBoolean("verified", false))
        }
        contacts.clear()
        contacts.addAll(list)
        list.forEach { online.putIfAbsent(it.key, Native.isOnline(sessionPtr, it.key)) }
    }

    fun openChat(key: String) {
        currentChat = key
        unread[key] = 0
        ops.execute { loadHistory(key) }
    }

    private fun loadHistory(key: String) {
        val arr = JSONArray(Native.history(sessionPtr, key))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Message(o.getLong("id"), o.getString("text"), o.getBoolean("mine"), o.getLong("time"), o.optString("status"))
        }
        messages[key] = list.toMutableStateList()
    }

    private fun log(key: String): SnapshotStateList<Message> =
        messages.getOrPut(key) { mutableListOf<Message>().toMutableStateList() }

    fun send(key: String, text: String) {
        if (text.isBlank()) return
        ops.execute {
            val id = Native.sendText(sessionPtr, key, text)
            if (id >= 0) {
                log(key).add(Message(id, text, mine = true, time = System.currentTimeMillis() / 1000, status = "sent"))
            }
        }
    }

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

    /** Применить одно событие из ядра. Вызывается из [pollThread] (фон). */
    private fun applyEvent(json: String) {
        if (json.isEmpty()) return
        val o = JSONObject(json)
        when (o.getString("type")) {
            "presence" -> {
                val peer = o.getString("peer")
                online[peer] = o.getBoolean("online")
                if (contacts.none { it.key == peer }) refreshContactsNow()
            }
            "text" -> {
                val peer = o.getString("peer")
                if (contacts.none { it.key == peer }) refreshContactsNow()
                log(peer).add(Message(o.getLong("id"), o.getString("text"), mine = false, time = o.getLong("time"), status = ""))
                if (currentChat != peer) unread[peer] = (unread[peer] ?: 0) + 1
            }
            "ack" -> updateStatus(o.getString("peer"), o.getLong("id"), "delivered")
            "undelivered" -> updateStatus(o.getString("peer"), o.getLong("id"), "failed")
            "call_invite" -> {
                val peer = o.getString("peer")
                val c = call
                // Свободны — обычный входящий. Тот же собеседник перезвонил —
                // переприглашение: гасим протухший звонок и показываем новый
                // входящий (так можно вернуться в звонок). От ДРУГОГО пира во
                // время звонка — игнор (заняты).
                if (c == null || c.peer == peer) {
                    if (c != null) endCall() // снять старое медиа/оверлей перед новым
                    call = CallState(peer, o.getLong("call_id"), incoming = true,
                        withVideo = o.optBoolean("video", false), status = "incoming")
                }
            }
            "call_accept" -> call?.let { c -> if (o.getString("peer") == c.peer) call = c.copy(status = "active") }
            "call_end" -> call?.let { c -> if (o.getString("peer") == c.peer) endCall() }
            "room_presence" -> if (o.getString("gid") == currentRoom) {
                // Мгновенно отражаем участника, не дожидаясь опроса тикером.
                val peer = o.getString("peer")
                if (o.optBoolean("left", false)) {
                    roomMembers.removeAll { it.peer == peer } // явный выход — убрать сразу
                } else {
                    val flags = o.optInt("flags", 0)
                    val rm = RoomMember(peer, flags and 0b01 != 0, flags and 0b10 != 0)
                    val idx = roomMembers.indexOfFirst { it.peer == peer }
                    if (idx >= 0) roomMembers[idx] = rm else roomMembers.add(rm)
                }
            }
            "groups_changed" -> {
                refreshGroupsNow()
                currentGroup?.let { loadGroupHistory(it) }
            }
            "group_msg" -> {
                val gid = o.getString("gid")
                groupLog(gid).add(GroupMsg(o.getString("text"), mine = false, time = System.currentTimeMillis() / 1000))
            }
        }
    }

    private fun updateStatus(peer: String, id: Long, status: String) {
        val l = messages[peer] ?: return
        val idx = l.indexOfFirst { it.id == id && it.mine }
        if (idx >= 0) l[idx] = l[idx].copy(status = status)
    }

    // ── Звонки ──────────────────────────────────────────────────────────────

    fun startOutgoing(peer: String, video: Boolean = false) {
        if (call != null) return
        // Оптимистично показываем оверлей сразу; параметры/старт медиа — в фоне.
        call = CallState(peer, 0, incoming = false, withVideo = video, status = "ringing")
        ops.execute {
            val vw = if (video) VIDEO_W else 0
            val vh = if (video) VIDEO_H else 0
            val params = JSONObject(Native.callStart(sessionPtr, peer, video, vw, vh))
            if (!params.has("call_id")) { call = null; return@execute }
            call = call?.copy(callId = params.getLong("call_id"))
            startMedia(params.getInt("sample_rate"), params.getInt("channels"), video)
        }
    }

    fun acceptCall() {
        val c = call ?: return
        if (!c.incoming) return
        call = c.copy(status = "active")
        ops.execute {
            val vw = if (c.withVideo) VIDEO_W else 0
            val vh = if (c.withVideo) VIDEO_H else 0
            val params = JSONObject(Native.callAccept(sessionPtr, c.peer, c.callId, c.withVideo, vw, vh))
            startMedia(params.optInt("sample_rate", 48000), params.optInt("channels", 1), c.withVideo)
        }
    }

    /** Запуск медиа (вызывается уже из фонового [ops]). */
    private fun startMedia(sampleRate: Int, channels: Int, video: Boolean) {
        runCatching { audio?.start(sampleRate, channels) }
        val ctx = appContext
        if (video && ctx != null) {
            runCatching {
                videoSender = VideoSender(ctx, VIDEO_W, VIDEO_H, VIDEO_FPS, VIDEO_BITRATE) { nal, keyframe ->
                    Native.callSendVideo(sessionPtr, nal, keyframe)
                }.also { it.start() }
            }
        }
    }

    fun hangUp() {
        val c = call ?: return
        endCall()
        ops.execute { Native.callEnd(sessionPtr, c.peer, c.callId) }
    }

    /** Локально завершить звонок: убрать оверлей (главный поток) + стоп медиа (фон). */
    private fun endCall() {
        val a = audio
        val vs = videoSender
        videoSender = null
        call = null
        Thread {
            runCatching { a?.stop() }
            runCatching { vs?.stop() }
        }.apply { isDaemon = true }.start()
    }

    fun toggleMute() {
        val c = call ?: return
        val m = !c.muted
        call = c.copy(muted = m)
        ops.execute { audio?.setMuted(m) }
    }

    // ── Комната-звонок (присутствие, Discord-style) ───────────────────────────

    /** Войти в комнату группы: маячок присутствия + мультипати аудио (микрофон
     *  всем участникам, приём от каждого с миксом). */
    fun joinRoom(gid: String, audio: Boolean = true, video: Boolean = false) {
        currentRoom = gid
        roomMembers.clear()
        val ctx = appContext
        ops.execute {
            Native.roomJoin(sessionPtr, gid, audio, video)
            val p = JSONObject(Native.roomAudioStart(sessionPtr, gid))
            val sr = p.optInt("sample_rate", 48000)
            val ch = p.optInt("channels", 1)
            if (ctx != null) {
                roomAudio = CallAudio(
                    ctx,
                    { data, n -> Native.roomPushAudio(sessionPtr, data, n) },
                    { out -> Native.roomPullAudio(sessionPtr, out) },
                    { m -> Native.roomSetMuted(sessionPtr, m) }
                ).also { it.start(sr, ch) }
            }
        }
    }

    /** Выйти из комнаты: стоп аудио + маячок «вышел». */
    fun leaveRoom() {
        val gid = currentRoom ?: return
        currentRoom = null
        roomMembers.clear()
        val a = roomAudio; roomAudio = null
        ops.execute { Native.roomLeave(sessionPtr, gid); Native.roomAudioStop(sessionPtr) }
        Thread { runCatching { a?.stop() } }.apply { isDaemon = true }.start()
    }

    // ── Группы ──────────────────────────────────────────────────────────────

    fun refreshGroups() = ops.execute { refreshGroupsNow() }

    private fun refreshGroupsNow() {
        val arr = JSONArray(Native.groupList(sessionPtr))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Group(o.getString("gid"), o.getString("name"), o.optInt("members", 1))
        }
        groups.clear()
        groups.addAll(list)
    }

    /** Создать группу (асинхронно). Появится в списке по готовности. */
    fun createGroup(name: String) = ops.execute {
        val gid = Native.groupCreate(sessionPtr, name.ifBlank { "Группа" })
        if (gid.isNotEmpty()) refreshGroupsNow()
    }

    fun openGroup(gid: String) {
        currentGroup = gid
        ops.execute { loadGroupHistory(gid) }
    }

    private fun loadGroupHistory(gid: String) {
        val arr = JSONArray(Native.groupHistory(sessionPtr, gid))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            GroupMsg(o.getString("text"), o.getBoolean("mine"), o.optLong("time", 0))
        }
        groupMessages[gid] = list.toMutableStateList()
    }

    private fun groupLog(gid: String): SnapshotStateList<GroupMsg> =
        groupMessages.getOrPut(gid) { mutableListOf<GroupMsg>().toMutableStateList() }

    fun inviteToGroup(gid: String, peerHex: String) = ops.execute { Native.groupInvite(sessionPtr, gid, peerHex) }

    fun sendGroupText(gid: String, text: String) {
        if (text.isBlank()) return
        ops.execute {
            if (Native.groupSendText(sessionPtr, gid, text)) {
                groupLog(gid).add(GroupMsg(text, mine = true, time = System.currentTimeMillis() / 1000))
            }
        }
    }

    // ── Диагностика: тест пропускной способности ─────────────────────────────

    /** Включить режим приёмника бенча (живая скорость в [benchRecvMbps]). */
    fun startBenchRecv() {
        benchRecv = true
        ops.execute { Native.benchRecvStart(sessionPtr) }
    }

    /** Блб ~30 МБ на пира; результат отправки в [benchResult]. */
    fun blastBench(peerHex: String) {
        benchResult = "отправка…"
        ops.execute {
            val r = JSONObject(Native.benchBlast(sessionPtr, peerHex, 30_000))
            benchResult = "отправлено %.1f Мбит/с (%.1f МБ)".format(
                r.optDouble("mbps", 0.0), r.optDouble("sent_bytes", 0.0) / 1e6,
            )
        }
    }
}
