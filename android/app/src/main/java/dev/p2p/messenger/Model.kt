package dev.p2p.messenger

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.toMutableStateList
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
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
    val key get() = "${width}x${height}@${fps}"

    companion object {
        val presets = listOf(
            VideoQuality("480p  30fps",  854,  480, 30, 1_000_000),
            VideoQuality("720p  30fps", 1280,  720, 30, 2_000_000),
            VideoQuality("720p  60fps", 1280,  720, 60, 4_000_000),
            VideoQuality("1080p 30fps", 1920, 1080, 30, 4_000_000),
            VideoQuality("1080p 60fps", 1920, 1080, 60, 8_000_000),
        )
        val default get() = presets[1] // 720p 30fps
    }
}

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
        /** Единственный экземпляр на процесс. Нативный узел/потоки тоже одни на
         *  процесс, поэтому модель должна переживать пересоздание Activity (иначе
         *  утекают второй узел и дублирующие poll/ticker-циклы → потеря событий). */
        val instance: AppModel by lazy { AppModel() }

        // TTL офлайн-очереди исходящих сообщений.
        // TODO: вынести в настройки приложения (v0.3+)
        private const val OFFLINE_QUEUE_TTL_SECS = 7L * 24 * 3600
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

    private fun loadSettings() {
        val prefs = appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE) ?: return
        val key = prefs.getString("video_quality", null) ?: return
        videoQuality = VideoQuality.presets.find { it.key == key } ?: VideoQuality.default
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
    @Volatile private var videoReceiver: VideoReceiver? = null

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
    @Volatile private var localMsgId = -1L  // отрицательные ID для «не отправлено» в логе

    /** Инициализация ядра. Звать с фонового потока (см. MainActivity).
     *  Идемпотентно: при пересоздании Activity повторно НЕ инициализирует. */
    fun start(dataDir: String) {
        if (started) return
        started = true
        loadSettings()
        val vaultKey = VaultKeyManager.getOrCreate(File(dataDir))
        val json = JSONObject(Native.start(dataDir, vaultKey))
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
        queueFile = File(dataDir, "offline_queue.json")
        loadOfflineQueue()
        restoreQueuedToLog()
        startLoops()
        registerNetworkCallback()
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
        }.apply { isDaemon = true; name = "native-poll"; start() }
        tickerThread = Thread {
            var tick = 0L
            var zeroPeerTicks = 0  // счётчик тиков без пиров
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
                    // Присутствие в группах, к которым мы НЕ подключены:
                    // показывает кто уже в комнате до того как мы вошли.
                    groups.forEach { g ->
                        if (g.gid == currentRoom) return@forEach
                        val arr = JSONArray(Native.roomPresencePeers(sessionPtr, g.gid))
                        groupPresence[g.gid] = (0 until arr.length()).map {
                            val o = arr.getJSONObject(it)
                            RoomMember(o.getString("peer"), o.optBoolean("audio"), o.optBoolean("video"))
                        }
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
                Thread.sleep(when {
                    benchRecv -> 1000
                    currentRoom != null -> 2000
                    foreground -> 3000
                    else -> 15000
                })
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
        }.toMutableList()
        // Queued messages (отрицательные localId) не сохраняются в БД — добавляем их обратно.
        synchronized(queueLock) { offlineQueueMap[key]?.toList() ?: emptyList() }
            .forEach { (text, ts) -> list.add(Message(--localMsgId, text, mine = true, time = ts, status = "queued")) }
        messages[key] = list.toMutableStateList()
    }

    private fun log(key: String): SnapshotStateList<Message> =
        messages.getOrPut(key) { mutableListOf<Message>().toMutableStateList() }

    fun send(key: String, text: String) {
        if (text.isBlank()) return
        ops.execute {
            val now = System.currentTimeMillis() / 1000
            val id = Native.sendText(sessionPtr, key, text)
            if (id >= 0) {
                // "pending" — Rust подтверждение ещё не пришло; сменится на "delivered" по ACK.
                log(key).add(Message(id, text, mine = true, time = now, status = "pending"))
            } else {
                // Пир недостижим — кладём в очередь, доставим при появлении онлайн.
                val localId = --localMsgId
                log(key).add(Message(localId, text, mine = true, time = now, status = "queued"))
                enqueueOffline(key, text, now)
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
                val wasOnline = online[peer]
                val nowOnline = o.getBoolean("online")
                online[peer] = nowOnline
                if (nowOnline && wasOnline != true) flushQueueFor(peer)
                if (contacts.none { it.key == peer }) refreshContactsNow()
            }
            "text" -> {
                val peer = o.getString("peer")
                if (contacts.none { it.key == peer }) refreshContactsNow()
                log(peer).add(Message(o.getLong("id"), o.getString("text"), mine = false, time = o.getLong("time"), status = ""))
                if (currentChat != peer) unread[peer] = (unread[peer] ?: 0) + 1
            }
            "ack" -> updateStatus(o.getString("peer"), o.getLong("id"), "delivered")
            "undelivered" -> {
                // Rust ретраил 5 минут и не смог — передаём в Kotlin-очередь.
                // Пир появится онлайн → flushQueueFor доставит автоматически.
                val peer = o.getString("peer")
                val id = o.getLong("id")
                val l = messages[peer] ?: return
                val idx = l.indexOfFirst { it.id == id && it.mine }
                if (idx >= 0) {
                    val msg = l[idx]
                    l[idx] = msg.copy(status = "queued")
                    enqueueOffline(peer, msg.text, msg.time)
                }
            }
            "room_invite" -> {
                val peer = o.getString("peer")
                val roomId = o.getString("room_id")
                val audio = o.optBoolean("audio", true)
                val video = o.optBoolean("video", false)
                val existing = incomingCall
                // Свободны — обычный входящий. Тот же собеседник перезванивает —
                // показываем новый экран входящего. От другого пира, пока заняты — игнор.
                if (existing == null || existing.peer == peer) {
                    if (currentRoom != null && dmCallPeer == peer) leaveRoom()
                    incomingCall = IncomingRoomCall(peer, roomId, audio, video)
                    val callerName = contacts.firstOrNull { it.key == peer }?.name ?: "Контакт"
                    appContext?.let { IncomingCallReceiver.show(it, callerName) }
                }
            }
            "room_decline" -> {
                val peer = o.getString("peer")
                if (peer == dmCallPeer) leaveRoom()
                // Звонящий сбросил до того как мы ответили — гасим уведомление.
                if (incomingCall?.peer == peer) {
                    incomingCall = null
                    appContext?.let { IncomingCallReceiver.cancel(it) }
                }
            }
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

    // ── Офлайн-очередь ────────────────────────────────────────────────────────

    private fun loadOfflineQueue() {
        val f = queueFile ?: return
        if (!f.exists()) return
        val cutoff = System.currentTimeMillis() / 1000 - OFFLINE_QUEUE_TTL_SECS
        try {
            val json = JSONObject(f.readText())
            synchronized(queueLock) {
                json.keys().forEach { peer ->
                    val arr = json.getJSONArray(peer)
                    val msgs = (0 until arr.length()).mapNotNull { i ->
                        val o = arr.getJSONObject(i)
                        val ts = o.getLong("ts")
                        if (ts > cutoff) Pair(o.getString("text"), ts) else null
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
        val name = contacts.firstOrNull { it.key == peer }?.name ?: ""
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

    /** Принять входящий DM-звонок. */
    fun acceptDmCall() {
        val ic = incomingCall ?: return
        incomingCall = null
        val ctx = appContext ?: return
        IncomingCallReceiver.cancel(ctx)
        val name = contacts.firstOrNull { it.key == ic.peer }?.name ?: ""
        ops.execute {
            val roomId = Native.roomJoinDm(sessionPtr, ic.peer, ic.audio, ic.video)
            if (roomId.isEmpty()) return@execute
            currentRoom = roomId
            dmCallPeer = ic.peer
            roomMembers.clear()
            CallService.start(ctx, name)
            startRoomMedia(ctx, roomId, ic.video)
        }
    }

    /** Отклонить входящий DM-звонок. */
    fun declineDmCall() {
        val ic = incomingCall ?: return
        incomingCall = null
        appContext?.let { IncomingCallReceiver.cancel(it) }
        ops.execute { Native.roomDeclineDm(sessionPtr, ic.peer) }
    }

    private fun startRoomMedia(ctx: android.content.Context, gid: String, video: Boolean) {
        val p = JSONObject(Native.roomAudioStart(sessionPtr, gid))
        val sr = p.optInt("sample_rate", 48000)
        val ch = p.optInt("channels", 1)
        roomAudio = CallAudio(
            ctx,
            { data, n -> Native.roomPushAudio(sessionPtr, data, n) },
            { out -> Native.roomPullAudio(sessionPtr, out) },
            { m -> Native.roomSetMuted(sessionPtr, m) }
        ).also { it.start(sr, ch) }
        if (video) {
            videoActive = true
            // Кап входящей видео-очереди в кадрах. Декодер пейсит вывод по wall-clock,
            // поэтому кап = постоянная задержка. Тяжёлые 1080p-кадры приходят пачками
            // (много фрагментов на меше) и требуют глубже буфер против ложных скипов;
            // лёгкие режимы держат низкую задержку. Привязка к разрешению, а не к fps:
            // запрошенный fps ненадёжен (камера 1080p часто отдаёт 30 вместо 60).
            val cap = when {
                videoQuality.height >= 1080 -> 24  // ~800мс при реальных 30fps
                videoQuality.height >= 720  -> 14
                else                        -> 9
            }
            Native.roomSetVideoCap(sessionPtr, cap)
            videoSender = VideoSender(
                ctx,
                videoQuality.width, videoQuality.height, videoQuality.fps, videoQuality.bitrate
            ) { data, key -> Native.roomPushVideo(sessionPtr, data, key) }.also { it.start() }
        }
    }

    /** Подключить Surface для отображения входящего видео (вызывается из UI). */
    fun attachVideoReceiver(peerHex: String, surface: android.view.Surface) {
        videoReceiver?.stop()
        videoReceiver = VideoReceiver(
            surface,
            videoQuality.width, videoQuality.height, videoQuality.fps
        ) { Native.roomPullVideoPacket(sessionPtr, peerHex) }.also { it.start() }
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
        // Запомнить ДО очистки: для DM-звонка нужно явно уведомить пира.
        val peer = dmCallPeer
        currentRoom = null
        dmCallPeer = null
        videoActive = false
        roomMembers.clear()
        val a = roomAudio; roomAudio = null
        val vs = videoSender; videoSender = null
        val vr = videoReceiver; videoReceiver = null
        val ctx = appContext
        ops.execute {
            // DM-звонок: явный сигнал пиру (отмена дозвона ИЛИ сброс активного
            // звонка). roomLeave шлёт маячок «вышел» только ЧЛЕНАМ комнаты, а при
            // отмене дозвона пир ещё не вступил → у него висел бы экран входящего.
            // RoomDecline у пира гасит incomingCall+уведомление, а если он уже в
            // звонке — завершает его (как «положили трубку»).
            if (peer != null) Native.roomDeclineDm(sessionPtr, peer)
            Native.roomLeave(sessionPtr, gid)
            Native.roomAudioStop(sessionPtr)
            if (ctx != null) CallService.stop(ctx)
        }
        Thread {
            runCatching { a?.stop() }
            runCatching { vs?.stop() }
            runCatching { vr?.stop() }
        }.apply { isDaemon = true }.start()
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

    /** Создать группу (fire-and-forget). Появится в списке по событию
     *  `groups_changed` (см. applyEvent → refreshGroupsNow). */
    fun createGroup(name: String) = ops.execute {
        Native.groupCreate(sessionPtr, name.ifBlank { "Группа" })
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
        // Оптимистично показываем своё сообщение сразу; отправка — fire-and-forget.
        groupLog(gid).add(GroupMsg(text, mine = true, time = System.currentTimeMillis() / 1000))
        ops.execute { Native.groupSendText(sessionPtr, gid, text) }
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
