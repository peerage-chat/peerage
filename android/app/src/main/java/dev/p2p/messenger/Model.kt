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
const val VIDEO_W = 640
const val VIDEO_H = 480
const val VIDEO_FPS = 24
const val VIDEO_BITRATE = 2_000_000

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
class AppModel {
    var me by mutableStateOf<Me?>(null)
    var error by mutableStateOf<String?>(null)

    val contacts = mutableStateListOf<Contact>()
    val online = mutableStateMapOf<String, Boolean>()
    val unread = mutableStateMapOf<String, Int>()
    val messages = mutableStateMapOf<String, SnapshotStateList<Message>>()

    var currentChat by mutableStateOf<String?>(null)
    var netPeers by mutableStateOf(0)
    /** Результат последней попытки добавить контакт ("" | added | known | error). */
    var addStatus by mutableStateOf("")

    // Группы (MLS).
    val groups = mutableStateListOf<Group>()
    val groupMessages = mutableStateMapOf<String, SnapshotStateList<GroupMsg>>()
    var currentGroup by mutableStateOf<String?>(null)

    // Звонок (1:1). audio/appContext задаются из Activity (нужен Context).
    var call by mutableStateOf<CallState?>(null)
    var audio: CallAudio? = null
    var appContext: android.content.Context? = null
    @Volatile private var videoSender: VideoSender? = null

    // Фоновое взаимодействие с ядром (никогда не на главном потоке).
    private val ops = Executors.newSingleThreadExecutor { Thread(it, "native-ops").apply { isDaemon = true } }
    @Volatile private var running = false
    private var pollThread: Thread? = null
    private var tickerThread: Thread? = null

    /** Инициализация ядра. Звать с фонового потока (см. MainActivity). */
    fun start(dataDir: String) {
        val json = JSONObject(Native.start(dataDir))
        if (json.has("error")) {
            error = json.getString("error")
            return
        }
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
                var e = Native.pollEvent()
                while (e.isNotEmpty()) {
                    runCatching { applyEvent(e) }
                    e = Native.pollEvent()
                }
                Thread.sleep(80)
            }
        }.apply { isDaemon = true; name = "native-poll"; start() }
        tickerThread = Thread {
            while (running) {
                runCatching {
                    netPeers = Native.netPeers()
                    contacts.map { it.key }.forEach { online[it] = Native.isOnline(it) }
                }
                Thread.sleep(3000)
            }
        }.apply { isDaemon = true; name = "native-ticker"; start() }
    }

    // ── Контакты / чат ────────────────────────────────────────────────────────

    fun refreshContacts() = ops.execute { refreshContactsNow() }

    private fun refreshContactsNow() {
        val arr = JSONArray(Native.listContacts())
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Contact(o.getString("key"), o.getString("name"), o.optString("fingerprint"), o.optBoolean("verified", false))
        }
        contacts.clear()
        contacts.addAll(list)
        list.forEach { online.putIfAbsent(it.key, Native.isOnline(it.key)) }
    }

    fun openChat(key: String) {
        currentChat = key
        unread[key] = 0
        ops.execute { loadHistory(key) }
    }

    private fun loadHistory(key: String) {
        val arr = JSONArray(Native.history(key))
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
            val id = Native.sendText(key, text)
            if (id >= 0) {
                log(key).add(Message(id, text, mine = true, time = System.currentTimeMillis() / 1000, status = "sent"))
            }
        }
    }

    fun addContact(key: String, name: String) = ops.execute {
        val r = Native.addContact(key, name)
        if (r == "added") refreshContactsNow()
        addStatus = r
    }

    fun toggleVerified(key: String) = ops.execute {
        val c = contacts.firstOrNull { it.key == key } ?: return@execute
        Native.setVerified(key, !c.verified)
        refreshContactsNow()
    }

    fun rename(key: String, name: String) = ops.execute {
        Native.renameContact(key, name)
        refreshContactsNow()
    }

    fun remove(key: String) = ops.execute {
        Native.removeContact(key)
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
            "call_invite" -> if (call == null) {
                call = CallState(o.getString("peer"), o.getLong("call_id"), incoming = true,
                    withVideo = o.optBoolean("video", false), status = "incoming")
            }
            "call_accept" -> call?.let { c -> if (o.getString("peer") == c.peer) call = c.copy(status = "active") }
            "call_end" -> call?.let { c -> if (o.getString("peer") == c.peer) endCall() }
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
            val params = JSONObject(Native.callStart(peer, video, vw, vh))
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
            val params = JSONObject(Native.callAccept(c.peer, c.callId, c.withVideo, vw, vh))
            startMedia(params.optInt("sample_rate", 48000), params.optInt("channels", 1), c.withVideo)
        }
    }

    /** Запуск медиа (вызывается уже из фонового [ops]). */
    private fun startMedia(sampleRate: Int, channels: Int, video: Boolean) {
        runCatching { audio?.start(sampleRate, channels) }
        val ctx = appContext
        if (video && ctx != null) {
            runCatching { videoSender = VideoSender(ctx, VIDEO_W, VIDEO_H, VIDEO_FPS, VIDEO_BITRATE).also { it.start() } }
        }
    }

    fun hangUp() {
        val c = call ?: return
        endCall()
        ops.execute { Native.callEnd(c.peer, c.callId) }
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

    // ── Группы ──────────────────────────────────────────────────────────────

    fun refreshGroups() = ops.execute { refreshGroupsNow() }

    private fun refreshGroupsNow() {
        val arr = JSONArray(Native.groupList())
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Group(o.getString("gid"), o.getString("name"), o.optInt("members", 1))
        }
        groups.clear()
        groups.addAll(list)
    }

    /** Создать группу (асинхронно). Появится в списке по готовности. */
    fun createGroup(name: String) = ops.execute {
        val gid = Native.groupCreate(name.ifBlank { "Группа" })
        if (gid.isNotEmpty()) refreshGroupsNow()
    }

    fun openGroup(gid: String) {
        currentGroup = gid
        ops.execute { loadGroupHistory(gid) }
    }

    private fun loadGroupHistory(gid: String) {
        val arr = JSONArray(Native.groupHistory(gid))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            GroupMsg(o.getString("text"), o.getBoolean("mine"), o.optLong("time", 0))
        }
        groupMessages[gid] = list.toMutableStateList()
    }

    private fun groupLog(gid: String): SnapshotStateList<GroupMsg> =
        groupMessages.getOrPut(gid) { mutableListOf<GroupMsg>().toMutableStateList() }

    fun inviteToGroup(gid: String, peerHex: String) = ops.execute { Native.groupInvite(gid, peerHex) }

    fun sendGroupText(gid: String, text: String) {
        if (text.isBlank()) return
        ops.execute {
            if (Native.groupSendText(gid, text)) {
                groupLog(gid).add(GroupMsg(text, mine = true, time = System.currentTimeMillis() / 1000))
            }
        }
    }
}
