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
 * Состояние приложения поверх [Native]. Держит контакты/историю/онлайн в
 * Compose-наблюдаемых структурах; источник истины — Rust-ядро, это его зеркало.
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

    // Группы (MLS).
    val groups = mutableStateListOf<Group>()
    val groupMessages = mutableStateMapOf<String, SnapshotStateList<GroupMsg>>()
    var currentGroup by mutableStateOf<String?>(null)

    // Звонок (1:1). audio/appContext задаются из Activity (нужен Context).
    var call by mutableStateOf<CallState?>(null)
    var audio: CallAudio? = null
    var appContext: android.content.Context? = null
    @Volatile private var videoSender: VideoSender? = null

    /** Инициализация ядра (блокирующая — звать с IO-потока). */
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
        refreshContacts()
        refreshGroups()
    }

    fun refreshContacts() {
        val arr = JSONArray(Native.listContacts())
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Contact(
                key = o.getString("key"),
                name = o.getString("name"),
                fingerprint = o.optString("fingerprint"),
                verified = o.optBoolean("verified", false),
            )
        }
        contacts.clear()
        contacts.addAll(list)
        list.forEach { online.putIfAbsent(it.key, Native.isOnline(it.key)) }
    }

    fun openChat(key: String) {
        currentChat = key
        unread[key] = 0
        loadHistory(key)
    }

    private fun loadHistory(key: String) {
        val arr = JSONArray(Native.history(key))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Message(
                id = o.getLong("id"),
                text = o.getString("text"),
                mine = o.getBoolean("mine"),
                time = o.getLong("time"),
                status = o.optString("status"),
            )
        }
        messages[key] = list.toMutableStateList()
    }

    private fun log(key: String): SnapshotStateList<Message> =
        messages.getOrPut(key) { mutableListOf<Message>().toMutableStateList() }

    fun send(key: String, text: String) {
        if (text.isBlank()) return
        val id = Native.sendText(key, text)
        if (id >= 0) {
            log(key).add(Message(id, text, mine = true, time = System.currentTimeMillis() / 1000, status = "sent"))
        }
    }

    fun addContact(key: String, name: String): String {
        val r = Native.addContact(key, name)
        if (r == "added") refreshContacts()
        return r
    }

    fun toggleVerified(key: String) {
        val c = contacts.firstOrNull { it.key == key } ?: return
        Native.setVerified(key, !c.verified)
        refreshContacts()
    }

    fun rename(key: String, name: String) {
        Native.renameContact(key, name)
        refreshContacts()
    }

    fun remove(key: String) {
        Native.removeContact(key)
        if (currentChat == key) currentChat = null
        refreshContacts()
    }

    /** Применить одно событие из ядра (звать на главном потоке). */
    fun applyEvent(json: String) {
        if (json.isEmpty()) return
        val o = JSONObject(json)
        when (o.getString("type")) {
            "presence" -> {
                val peer = o.getString("peer")
                online[peer] = o.getBoolean("online")
                if (contacts.none { it.key == peer }) refreshContacts()
            }
            "text" -> {
                val peer = o.getString("peer")
                if (contacts.none { it.key == peer }) refreshContacts()
                log(peer).add(
                    Message(o.getLong("id"), o.getString("text"), mine = false, time = o.getLong("time"), status = "")
                )
                if (currentChat != peer) unread[peer] = (unread[peer] ?: 0) + 1
            }
            "ack" -> updateStatus(o.getString("peer"), o.getLong("id"), "delivered")
            "undelivered" -> updateStatus(o.getString("peer"), o.getLong("id"), "failed")
            "call_invite" -> if (call == null) {
                call = CallState(
                    peer = o.getString("peer"),
                    callId = o.getLong("call_id"),
                    incoming = true,
                    withVideo = o.optBoolean("video", false),
                    status = "incoming",
                )
            }
            "call_accept" -> call?.let { c ->
                if (o.getString("peer") == c.peer) call = c.copy(status = "active")
            }
            "call_end" -> call?.let { c ->
                if (o.getString("peer") == c.peer) endCall()
            }
            "groups_changed" -> {
                refreshGroups()
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

    /** Начать исходящий звонок (аудио или видео). */
    fun startOutgoing(peer: String, video: Boolean = false) {
        if (call != null) return
        val vw = if (video) VIDEO_W else 0
        val vh = if (video) VIDEO_H else 0
        val params = JSONObject(Native.callStart(peer, video, vw, vh))
        if (!params.has("call_id")) return
        call = CallState(peer, params.getLong("call_id"), incoming = false, withVideo = video, status = "ringing")
        // Старт медиа (MediaCodec/AudioRecord) — НЕ на UI-потоке (блокирующие операции).
        startMedia(params.getInt("sample_rate"), params.getInt("channels"), video)
    }

    /** Принять входящий звонок. */
    fun acceptCall() {
        val c = call ?: return
        if (!c.incoming) return
        val vw = if (c.withVideo) VIDEO_W else 0
        val vh = if (c.withVideo) VIDEO_H else 0
        val params = JSONObject(Native.callAccept(c.peer, c.callId, c.withVideo, vw, vh))
        call = c.copy(status = "active")
        startMedia(params.optInt("sample_rate", 48000), params.optInt("channels", 1), c.withVideo)
    }

    /** Запуск аудио/видео в фоне — инициализация MediaCodec/AudioRecord блокирует. */
    private fun startMedia(sampleRate: Int, channels: Int, video: Boolean) {
        val ctx = appContext
        Thread {
            runCatching { audio?.start(sampleRate, channels) }
            if (video && ctx != null) {
                runCatching {
                    videoSender = VideoSender(ctx, VIDEO_W, VIDEO_H, VIDEO_FPS, VIDEO_BITRATE).also { it.start() }
                }
            }
        }.start()
    }

    /** Завершить/отклонить звонок (шлёт end собеседнику и глушит аудио). */
    fun hangUp() {
        val c = call ?: return
        Native.callEnd(c.peer, c.callId)
        endCall()
    }

    /** Локально завершить звонок (без отправки end — например, по событию call_end). */
    private fun endCall() {
        val a = audio
        val vs = videoSender
        videoSender = null
        call = null // UI: убрать оверлей сразу (на главном потоке)
        // Остановка медиа (MediaCodec.release/AudioTrack — блокирующие) — в фоне,
        // иначе UI-поток виснет → ANR.
        Thread {
            runCatching { a?.stop() }
            runCatching { vs?.stop() }
        }.start()
    }

    fun toggleMute() {
        val c = call ?: return
        val m = !c.muted
        audio?.setMuted(m)
        call = c.copy(muted = m)
    }

    // ── Группы ──────────────────────────────────────────────────────────────

    fun refreshGroups() {
        val arr = JSONArray(Native.groupList())
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Group(o.getString("gid"), o.getString("name"), o.optInt("members", 1))
        }
        groups.clear()
        groups.addAll(list)
    }

    fun createGroup(name: String): String {
        val gid = Native.groupCreate(name.ifBlank { "Группа" })
        if (gid.isNotEmpty()) refreshGroups()
        return gid
    }

    fun openGroup(gid: String) {
        currentGroup = gid
        loadGroupHistory(gid)
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

    fun inviteToGroup(gid: String, peerHex: String) {
        Native.groupInvite(gid, peerHex)
    }

    fun sendGroupText(gid: String, text: String) {
        if (text.isBlank()) return
        if (Native.groupSendText(gid, text)) {
            groupLog(gid).add(GroupMsg(text, mine = true, time = System.currentTimeMillis() / 1000))
        }
    }
}
