package dev.p2p.messenger

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

data class Group(val gid: String, val name: String, val members: Int, val admin: Boolean = false)

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
    val text: String,
    val mine: Boolean,
    val time: Long,
    /** hex ed25519-pubkey отправителя (имя/цвет/аватарка в UI). "" — неизвестно. */
    val sender: String = "",
    /** Системная строка (смена состава) — рендерится отдельно. */
    val system: Boolean = false,
)

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

    private fun loadSettings() {
        val prefs = appContext?.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE) ?: return
        showTechInfo = prefs.getBoolean("tech_info", false)
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
    fun start(dataDir: String) {
        if (started) return
        started = true
        loadSettings()
        loadAudioPrefs()
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
                    if (benchRecv) {
                        benchRecvMbps = JSONObject(Native.benchRecvStats(sessionPtr)).optDouble("mbps", 0.0)
                    }
                    // Снапшот стата сети — только на переднем плане (экран «Сеть»).
                    if (foreground) {
                        val o = JSONObject(Native.netStats(sessionPtr))
                        val pa = o.optJSONArray("peers") ?: JSONArray()
                        val peers = (0 until pa.length()).map { i ->
                            val p = pa.getJSONObject(i)
                            NetPeer(
                                p.optString("key"),
                                p.optBoolean("inbound"),
                                p.optBoolean("up"),
                                p.optDouble("latency_ms"),
                                p.optLong("rx_rate"),
                                p.optLong("tx_rate"),
                                p.optDouble("uptime_s"),
                            )
                        }
                        netStats = NetStats(
                            o.optInt("peer_count"),
                            o.optInt("tree"),
                            o.optDouble("ping_ms"),
                            o.optLong("rx_rate"),
                            o.optLong("tx_rate"),
                            peers,
                        )
                    }
                    // Присутствие открытой группы «N уч · M в сети» — агрегат
                    // (ростер ∩ онлайн) считает движок, клиент лишь рендерит.
                    currentGroup?.let { gid ->
                        val o = JSONObject(Native.groupPresence(sessionPtr, gid))
                        currentGroupRoster = GroupRoster(o.optInt("total", 1), o.optInt("online", 1))
                    }
                    // Присутствие в комнате: кто сейчас внутри (по свежим маячкам).
                    currentRoom?.let { gid ->
                        val arr = JSONArray(Native.roomParticipants(sessionPtr, gid))
                        val list = (0 until arr.length()).map {
                            val o = arr.getJSONObject(it)
                            RoomMember(o.getString("peer"), o.optBoolean("audio"), o.optBoolean("video"))
                        }
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
                        val arr = JSONArray(Native.roomPresencePeers(sessionPtr, g.gid))
                        val list = (0 until arr.length()).map {
                            val o = arr.getJSONObject(it)
                            RoomMember(o.getString("peer"), o.optBoolean("audio"), o.optBoolean("video"))
                        }
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
                        val arr = JSONArray(Native.roomSpeakingPeers(sessionPtr))
                        val set = (0 until arr.length()).map { arr.getString(it) }.toSet()
                        if (speakingPeers != set) speakingPeers = set
                        // Трафик: скорость = дельта байт × 8 / dt.
                        val tj = JSONObject(Native.roomTrafficJson(sessionPtr))
                        val now = System.currentTimeMillis()
                        val dt = ((now - prevT).coerceAtLeast(1)) / 1000.0
                        val cur = mutableMapOf<String, Long>()
                        val kbps = mutableMapOf<String, Int>()
                        tj.keys().forEach { k ->
                            val b = tj.getLong(k)
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
                // Стабильная комната: если мы УЖЕ в этой комнате с этим пиром, он
                // просто перезашёл — остаёмся в звонке, его возврат отразит presence.
                // НЕ рвём звонок и не показываем промпт заново (иначе teardown +
                // каскад decline — это и было «реентри ломает звонок»).
                if (currentRoom == roomId && dmCallPeer == peer) return
                val existing = incomingCall
                // Свободны — обычный входящий. Тот же собеседник перезванивает —
                // обновляем экран входящего. От другого пира, пока заняты — игнор.
                if (existing == null || existing.peer == peer) {
                    incomingCall = IncomingRoomCall(peer, roomId, audio, video)
                    val callerName = contacts.firstOrNull { it.key == peer }?.name
                        ?: appContext?.getString(R.string.common_contact).orEmpty()
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
                currentGroup?.let {
                    loadGroupHistory(it)
                    loadGroupMembers(it)
                }
            }
            "group_removed" -> {
                // ADR-004: нас отозвали. Если открыт этот чат — выходим и уведомляем.
                val gid = o.optString("gid")
                if (currentGroup == gid) currentGroup = null
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
            "group_msg" -> {
                val gid = o.getString("gid")
                groupLog(gid).add(
                    GroupMsg(
                        o.optString("text"),
                        mine = false,
                        time = System.currentTimeMillis() / 1000,
                        sender = o.optString("sender"),
                        system = o.optBoolean("system", false),
                    ),
                )
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

    /** Принять входящий DM-звонок с ПРЕСЕТОМ своего mic/video (выбор join'ера до
     *  входа, НЕ флаги звонящего). Дефолт — мик вкл, видео выкл. */
    fun acceptDmCall(audio: Boolean = true, video: Boolean = false) {
        val ic = incomingCall ?: return
        incomingCall = null
        val ctx = appContext ?: return
        IncomingCallReceiver.cancel(ctx)
        val name = contacts.firstOrNull { it.key == ic.peer }?.name ?: ""
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
        val p = JSONObject(Native.roomAudioStart(sessionPtr, gid, audioBitrate, audioBitrate >= 32_000))
        val sr = p.optInt("sample_rate", 48000)
        val ch = p.optInt("channels", 1)
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
        val arr = JSONArray(Native.groupList(sessionPtr))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Group(o.getString("gid"), o.getString("name"), o.optInt("members", 1), o.optBoolean("admin", false))
        }
        groups.clear()
        groups.addAll(list)
    }

    /** Ключи участников группы (hex, кроме себя) — для экрана управления. */
    val groupMemberKeys = mutableStateMapOf<String, List<String>>()

    /** Мы ли админ группы (гейт кнопки «удалить»). */
    fun isGroupAdmin(gid: String): Boolean = groups.firstOrNull { it.gid == gid }?.admin == true

    /** Имя для показа по hex-ключу: имя контакта либо укороченный ключ. */
    fun peerName(hex: String): String =
        contacts.firstOrNull { it.key == hex }?.name ?: (hex.take(8) + "…")

    fun loadGroupMembers(gid: String) = ops.execute {
        val arr = JSONArray(Native.groupMembers(sessionPtr, gid))
        groupMemberKeys[gid] = (0 until arr.length()).map { arr.getString(it) }
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
        val arr = JSONArray(Native.groupHistory(sessionPtr, gid))
        val list = (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            GroupMsg(
                o.optString("text"),
                o.getBoolean("mine"),
                o.optLong("time", 0),
                sender = o.optString("sender"),
                system = o.optBoolean("system", false),
            )
        }
        groupMessages[gid] = list.toMutableStateList()
    }

    private fun groupLog(gid: String): SnapshotStateList<GroupMsg> =
        groupMessages.getOrPut(gid) { mutableListOf<GroupMsg>().toMutableStateList() }

    fun inviteToGroup(gid: String, peerHex: String) = ops.execute { Native.groupInvite(sessionPtr, gid, peerHex) }

    fun sendGroupText(gid: String, text: String) {
        if (text.isBlank()) return
        // Оптимистично показываем своё сообщение сразу; отправка — fire-and-forget.
        groupLog(gid).add(
            GroupMsg(
                text,
                mine = true,
                time = System.currentTimeMillis() / 1000,
                sender = me?.pubkey ?: "",
            ),
        )
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
        benchResult = appContext?.getString(R.string.bench_sending).orEmpty()
        ops.execute {
            val r = JSONObject(Native.benchBlast(sessionPtr, peerHex, 30_000))
            benchResult = appContext?.getString(
                R.string.bench_sent,
                r.optDouble("mbps", 0.0),
                r.optDouble("sent_bytes", 0.0) / 1e6,
            ).orEmpty()
        }
    }
}
