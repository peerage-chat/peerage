package chat.peerage

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
/** Ошибка старта движка: данные зашифрованы другим ключом (`START_VAULT_MISMATCH`). */
internal const val START_VAULT_MISMATCH = "vault_mismatch"

/** Докачанный блоб крупнее этого в память не читается: из памяти рисуются фото,
 *  голосовое и аватары, а видео и файлы открываются выгрузкой в файл. */
internal const val INLINE_BLOB_MAX = 16L * 1024 * 1024

/** Почему данные не открылись (R-029). */
enum class Recovery {
    /** Keystore потерял ключ — обёртку ключа хранилища развернуть нечем. */
    KEY_LOST,

    /** Ключ есть, но данные зашифрованы другим (файл ключа пропал, данные чужие). */
    DATA_UNREADABLE,
}

enum class RecoverResult { OK, BAD_PHRASE, FAILED }

/** Сколько показывать «печатает…» после последнего сигнала (отправитель — раз в 3 с). */
internal const val TYPING_SHOWN_MS = 5_000L

class AppModel private constructor() {
    companion object {
        /** Начало сообщения в строке о реакции — не длиннее (символов). */
        private const val QUOTE_MAX = 40

        private const val KEY_SEEN_INVITES = "seen_invites"

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

        /** Нижний предел битрейта звука в «Авто», бит/с — тот же, что `MIN_AUDIO_BITRATE`
         *  движка: ниже речь рассыпается. Только для подписи режимов. */
        const val AUDIO_BITRATE_MIN = 16_000

        /** Шаг вариантов битрейта видео: 0,5 Мбит/с — мельче разница не видна. */
        /** Оценка входящего трафика звонка на участника, МБ/мин (call-ui.md): видео
         *  ≈30, голос ≈2. Одна пара чисел для входящего экрана и чипа звонка. */
        const val TRAFFIC_VIDEO_MB_MIN = 30
        const val TRAFFIC_AUDIO_MB_MIN = 2
        const val VIDEO_BITRATE_STEP = 500_000

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

    /** Что известно о копиях ключа и данных — от этого зависят полосы-напоминания на
     *  главной ([BackupFacts.strip]). Локальные факты, не секрет. */
    var backupFacts by mutableStateOf(BackupFacts())
        private set

    /** Копия ключа есть — значит, хоть раз прошла проверку трёх слов (не «нажал Записал»). */
    val keyBackupDone get() = backupFacts.keyDone

    private fun updateBackup(change: BackupFacts.(Long) -> BackupFacts) {
        backupFacts = backupFacts.change(System.currentTimeMillis() / 1000)
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)?.let { backupFacts.save(it) }
    }

    /** «Позже» на полосе-напоминании. */
    fun backupLater(s: BackupStrip) = updateBackup { later(s, it) }

    /** Ключ показан целиком. После проваленной проверки это и есть «сверить»: полоса
     *  «не пройдена» вернётся завтра, если проверка так и не пройдёт. */
    fun keyReconciled() {
        if (backupFacts.keyFailed) updateBackup { reconciled(it) }
    }

    /** Проверка копии ключа: совпали ли слова на номерах (с 1). Сама фраза в UI не
     *  приходит. Итог записывается здесь же: успех — копия есть, лестница дальше;
     *  [final] и не совпало — проверка не пройдена. */
    fun checkKeyWords(picks: List<Pair<Int, String>>, final: Boolean, onResult: (Boolean) -> Unit) = ops.execute {
        val ok = Native.checkSeedWords(sessionPtr, picks.joinToString(" ") { "${it.first} ${it.second}" })
        onMain {
            if (ok) {
                updateBackup { checkPassed(it) }
            } else if (final) {
                updateBackup { checkFailed(it) }
            }
            onResult(ok)
        }
    }

    /** Словарь BIP39 — подсказки при вводе слов. Грузится один раз, через [ops]. */
    var bip39 by mutableStateOf<List<String>>(emptyList())
        private set

    fun loadBip39() {
        if (bip39.isEmpty()) ops.execute { Native.bip39Words().split(" ").let { w -> onMain { bip39 = w } } }
    }

    fun clearRevealedPhrase() {
        revealedPhrase = null
    }

    /** Почему данные не открылись (R-029); null — открылись. */
    var recovery by mutableStateOf<Recovery?>(null)
        private set

    /** Ключ хранилища, которым движок не смог прочитать данные. Он уже задан
     *  процессу и годится для новых данных после карантина. Нет его — значит
     *  Keystore ключ потерял, и заводится новый. */
    private var recoveryKey: ByteArray? = null

    /** «Начать заново» (R-029): отложить нечитаемое; вызывающий перезапускает процесс. */
    fun recoverStartOver(dataDir: String, onDone: (Boolean) -> Unit) = ops.execute {
        val ok = Native.quarantineData(dataDir)
        if (ok) {
            if (recoveryKey == null) VaultKeyManager.reset(File(dataDir))
            // Новая личность — заново и онбординг с резервной копией фразы. commit():
            // процесс сейчас перезапустится, и apply() не успел бы записать.
            appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
                ?.edit()?.putBoolean("onboarded", false)?.putBoolean("key_backup_done", false)?.commit()
        }
        android.os.Handler(android.os.Looper.getMainLooper()).post { onDone(ok) }
    }

    /** «Восстановить по 24 словам» (R-029). Фраза проверяется ДО карантина: с
     *  опечаткой данные остаются на месте. Успех — вызывающий перезапускает процесс. */
    fun recoverFromPhrase(dataDir: String, phrase: String, onDone: (RecoverResult) -> Unit) = ops.execute {
        val dir = File(dataDir)
        val result = when {
            !Native.seedPhraseValid(phrase) -> RecoverResult.BAD_PHRASE
            !Native.quarantineData(dataDir) -> RecoverResult.FAILED
            else -> {
                val key = recoveryKey ?: run {
                    VaultKeyManager.reset(dir)
                    runCatching { VaultKeyManager.getOrCreate(dir) }.getOrNull()
                }
                if (key != null && Native.restoreSeedPhrase(dataDir, key, phrase)) {
                    // Фраза у человека есть — он её только что ввёл.
                    appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
                        ?.edit()?.putBoolean("onboarded", true)?.putBoolean("key_backup_done", true)
                        // После перезапуска — «Перенести переписку» (TransferFlow), один раз.
                        ?.putBoolean("offer_transfer", true)?.commit()
                    RecoverResult.OK
                } else {
                    RecoverResult.FAILED
                }
            }
        }
        android.os.Handler(android.os.Looper.getMainLooper()).post { onDone(result) }
    }

    /** Импорт личности из фразы (M4) на [ops], результат на главном потоке. Сумма фразы
     *  проверяется отдельно — «не сходится» и «не записалось» человеку говорятся по-разному.
     *  Успех: онбординг пройден и копия ключа есть (фраза только что введена); вызывающий
     *  перезапускает процесс. commit(), не apply(): тот не успевал на диск до System.exit. */
    fun importIdentity(dataDir: String, phrase: String, onResult: (RecoverResult) -> Unit) = ops.execute {
        val result = when {
            !Native.seedPhraseValid(phrase) -> RecoverResult.BAD_PHRASE
            !Native.importSeedPhrase(dataDir, phrase) -> RecoverResult.FAILED
            else -> {
                appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
                    ?.edit()?.putBoolean("onboarded", true)?.putBoolean("key_backup_done", true)
                    // После перезапуска — «Перенести переписку» (TransferFlow), один раз.
                    ?.putBoolean("offer_transfer", true)?.commit()
                RecoverResult.OK
            }
        }
        android.os.Handler(android.os.Looper.getMainLooper()).post { onResult(result) }
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

    /** «Убрать фото»: движок разошлёт контактам профиль без аватара. */
    fun clearSelfAvatar() = ops.execute {
        Native.clearSelfAvatar(sessionPtr)
        selfAvatar = null
    }

    /** Задать своё имя (M2): движок сохранит и разошлёт контактам. */
    fun updateSelfName(name: String) = ops.execute {
        Native.setSelfName(sessionPtr, name.trim())
        selfName = Native.selfName(sessionPtr)
        contactCode = Native.myContactCode(sessionPtr) // имя едет в коде
    }

    /** Байты аватаров контактов по hex-хэшу (M2). Заполняется при обновлении контактов;
     *  UI читает [Contact.avatarId] → сюда. */
    val contactAvatars = mutableStateMapOf<String, ByteArray>()

    /** Байты вложений чата по hex-хэшу блоба (M2). Реактивный кэш: при докачке
     *  (blob_received) картинка появляется в пузыре без ручного рефреша.
     *
     *  Читать напрямую НЕ надо — только через [blob]: карта ограничена по объёму и
     *  вытесняет давнее, а [blob] умеет заказать выселенное обратно. */
    val mediaBlobs = mutableStateMapOf<String, ByteArray>()

    /** Потолок кэша вложений в памяти. Раньше его не было вовсе: чат на два десятка
     *  фотографий держал в куче все их байты разом и никогда не отпускал. */
    private val blobCacheLimit = 24L * 1024 * 1024

    /** Порядок обращений для вытеснения (в конце — самые свежие). */
    internal val blobOrder = LinkedHashSet<String>()
    internal var blobBytes = 0L

    /** Блобы, чтение которых уже заказано — чтобы промах не плодил очередь заказов
     *  на каждую рекомпозицию. */
    private val blobWanted = mutableSetOf<String>()

    /**
     * Байты вложения для показа.
     *
     * При промахе заказывает чтение с диска и возвращает null: вызывающий рисует
     * заглушку, а байты приедут в реактивную карту и он перерисуется сам.
     */
    fun blob(blobId: String): ByteArray? {
        if (blobId.isEmpty()) return null
        val hit = mediaBlobs[blobId]
        if (hit != null) {
            synchronized(blobOrder) {
                blobOrder.remove(blobId)
                blobOrder.add(blobId)
            }
            return hit
        }
        val fresh = synchronized(blobWanted) { blobWanted.add(blobId) }
        if (fresh) reads.execute { cacheBlob(blobId) }
        return null
    }

    /** Вложения, докачанные за эту сессию: видео и файлы в память не читаются,
     *  и «скачано» для пузыря берётся отсюда (и из `downloaded` истории). */
    val savedBlobs = mutableStateMapOf<String, Boolean>()

    /**
     * Расшифровать вложение в файл `cacheDir/<dir>/` потоково и отдать путь на
     * главный поток (`null` — не вышло). Так открывают и проигрывают видео и
     * файлы: целиком в память они не поднимаются. Файл — открытым текстом, поэтому
     * вызывающий удаляет его, когда он больше не нужен; забытое чистится на старте.
     */
    fun exportBlob(blobId: String, dir: String, name: String, onReady: (java.io.File?) -> Unit) {
        val ctx = appContext ?: return onReady(null)
        heavy.execute {
            val folder = File(ctx.cacheDir, dir).apply { mkdirs() }
            // Имя из сообщения — чужая строка: оставляем только её последний сегмент.
            val safe = File(name.ifBlank { blobId.take(16) }).name.ifBlank { blobId.take(16) }
            val f = File(folder, safe)
            val ok = Native.blobExport(sessionPtr, blobId, f.absolutePath)
            android.os.Handler(android.os.Looper.getMainLooper()).post { onReady(if (ok) f else null) }
        }
    }

    /** Вложение — в общие «Загрузки» (п. 60); итог — тостом. */
    fun saveToDownloads(m: Message) {
        val ctx = appContext ?: return
        exportBlob(m.blobId, "export", m.name) { f ->
            heavy.execute {
                val name = f?.name.orEmpty()
                val ok = f != null && Downloads.save(ctx, f, name, m.mime)
                f?.delete()
                android.os.Handler(android.os.Looper.getMainLooper()).post {
                    android.widget.Toast.makeText(
                        ctx,
                        if (ok) ctx.getString(R.string.saved_to_downloads, name) else ctx.getString(R.string.viewer_save_failed),
                        android.widget.Toast.LENGTH_SHORT,
                    ).show()
                }
            }
        }
    }

    /** Подтянуть байты блоба в кэш, если он уже есть локально (на [ops]). */
    private fun cacheBlob(blobId: String) {
        if (blobId.isNotEmpty() && !mediaBlobs.containsKey(blobId)) {
            Native.blobRead(sessionPtr, blobId).takeIf { it.isNotEmpty() }?.let { putBlob(blobId, it) }
        }
        synchronized(blobWanted) { blobWanted.remove(blobId) }
    }

    /** Положить блоб в кэш, вытеснив самые давние, если вышли за потолок. */
    private fun putBlob(blobId: String, data: ByteArray) {
        val evict = synchronized(blobOrder) {
            if (blobOrder.add(blobId)) blobBytes += data.size
            val doomed = mutableListOf<String>()
            val it = blobOrder.iterator()
            // Свежий блоб не трогаем даже когда он один переполняет кэш: его сейчас
            // и показывают, а выселить его — значит тут же заказать снова.
            while (blobBytes > blobCacheLimit && it.hasNext()) {
                val old = it.next()
                if (old == blobId) continue
                it.remove()
                blobBytes -= (mediaBlobs[old]?.size ?: 0).toLong()
                doomed += old
            }
            doomed
        }
        mediaBlobs[blobId] = data
        evict.forEach { mediaBlobs.remove(it) }
    }

    val contacts = mutableStateListOf<Contact>()

    /** Входящие запросы в контакты от незнакомцев (гейт). */
    val contactRequests = mutableStateListOf<ContactRequest>()

    /** «Заблокированные» (5.2), новые сверху. */
    val blocked = mutableStateListOf<BlockedPeer>()
    val online = mutableStateMapOf<String, Boolean>()

    /** У контакта открыто приложение (п. 9, «в сети»); без связи не бывает. */
    val inApp = mutableStateMapOf<String, Boolean>()

    /**
     * Статус человека для точки и подписи: «в сети» → «на связи» → «не в сети». ЕДИНСТВЕННЫЙ
     * источник присутствия человека в интерфейсе: личный чат, лента и состав группы, списки
     * людей — все спрашивают здесь (п. 26). Себя я вижу «в сети», пока есть сеть.
     */
    fun presence(key: String): Presence = when {
        key == me?.pubkey -> if (offline) Presence.Offline else Presence.InApp
        online[key] != true -> Presence.Offline
        inApp[key] == true -> Presence.InApp
        else -> Presence.Reachable
    }
    val unread = mutableStateMapOf<String, Int>()

    /**
     * Ленты всех чатов: ключ — [chatKey], то есть вид чата входит в ключ. Раньше
     * карт было две (личные и групповые), и всё, что работает с историей, шло по
     * два раза — по одному на каждую.
     *
     * Читать удобнее через [chat]: он не заставляет вызывающего собирать ключ.
     */
    val messages = mutableStateMapOf<String, SnapshotStateList<Message>>()

    /** Лента чата: `chat(hex)` — личный, `chat(gid, group = true)` — групповой. Без
     *  удалённого, чьё удаление ещё можно отменить ([hidden]). */
    fun chat(id: String?, group: Boolean = false): List<Message> {
        val key = id?.let { chatKey(it, group) } ?: return emptyList()
        val list = messages[key].orEmpty()
        val gone = hidden[key] ?: return list
        return list.filter { MsgRef(it.id, it.sender) !in gone }
    }

    /** Удалённое «у себя», пока идёт отсчёт «Отменить»: уже не видно, но в движке ещё
     *  лежит. Ключ — [chatKey]. Удалённое не возвращается, поэтому после
     *  выполнения ссылки здесь не чистим — они просто ни на что не указывают. */
    private val hidden = mutableStateMapOf<String, Set<MsgRef>>()

    /** «Отменить» в снекбаре — один слот на приложение. */
    val undo = UndoSlot()

    /** Спрятать [refs] сразу, удалить через [del] по истечении отсчёта, вернуть по «Отменить». */
    internal fun deleteWithUndo(key: String, refs: List<MsgRef>, plural: Int, del: () -> Unit) {
        hidden[key] = hidden[key].orEmpty() + refs
        val text = appContext?.resources?.getQuantityString(plural, refs.size, refs.size).orEmpty()
        undo.offer(text, commit = { ops.execute(del) }, revert = { hidden[key] = hidden[key].orEmpty() - refs.toSet() })
    }

    var currentChat by mutableStateOf<String?>(null)
    var netPeers by mutableStateOf(0)

    /** Снапшот стата сети (экран «Сеть»). Обновляется тикером на переднем плане. */
    var netStats by mutableStateOf<NetStats?>(null)

    /** Узлы входа (экран «Узлы входа», строка в «Сети»). null — ещё не прочитаны. */
    var peerSettings by mutableStateOf<PeerSettings?>(null)
        private set

    fun refreshPeerSettings() = ops.execute { loadPeerSettings() }

    private fun loadPeerSettings() {
        val d = decodeOrNull<PeerSettingsDto>(Native.peerSettings(sessionPtr)) ?: return
        val s = PeerSettings(d.builtinOn, d.builtin, d.own)
        onMain { peerSettings = s }
    }

    /** Добавить свой узел входа; итог ("ok" | "invalid" | "duplicate") — в [done]. */
    fun addEntryNode(uri: String, done: (String) -> Unit) = ops.execute {
        val r = Native.peersAdd(sessionPtr, uri)
        if (r == "ok") loadPeerSettings()
        onMain { done(r) }
    }

    fun removeEntryNode(uri: String) = ops.execute {
        Native.peersRemove(sessionPtr, uri)
        loadPeerSettings()
    }

    fun setBuiltinNodes(on: Boolean) = ops.execute {
        Native.peersSetBuiltin(sessionPtr, on)
        loadPeerSettings()
    }

    // Диагностика: тест пропускной способности меша.
    var benchRecv by mutableStateOf(false) // режим приёмника включён
    var benchRecvMbps by mutableStateOf(0.0) // живая скорость приёма
    var benchResult by mutableStateOf("") // результат отправки (для blast)

    // Группы (MLS).
    val groups = mutableStateListOf<Group>()

    /** Входящие приглашения в группы (карточки на одобрение). */
    val groupInvites = mutableStateListOf<GroupInvite>()

    /** Непрочитанные в группах: gid → счётчик. Живёт только в памяти, как и [unread]
     *  у личных чатов — это «что пришло, пока я смотрел в другое место», а не факт
     *  переписки, который стоило бы хранить. */
    val groupUnread = mutableStateMapOf<String, Int>()
    var currentGroup by mutableStateOf<String?>(null)

    /** Сколько в группе людей и кто из них в сети / на связи — по [presence] каждого (п. 26).
     *  Пока состав не загружен — только число из списка групп. */
    fun groupHeadcount(gid: String): GroupHeadcount {
        val keys = groupMemberKeys[gid]
            ?: return GroupHeadcount(groups.firstOrNull { it.gid == gid }?.members ?: 1, 1, 0)
        val all = (keys + listOfNotNull(me?.pubkey)).distinct().map(::presence)
        return GroupHeadcount(all.size, all.count { it == Presence.InApp }, all.count { it == Presence.Reachable })
    }

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

    // Видео по умолчанию для звонков — загружается из SharedPreferences при start().
    // Меняется в «Настройки → Медиа и звонки» и при «Применить» на экране «Показать
    // видео» в звонке; оба пути пишут одно и то же.
    var videoQuality by mutableStateOf(VideoQuality.default)
        private set

    /** Битрейт видео: «авто» (рекомендация под разрешение/fps) или вариант 0/1/2 из
     *  [bitrateOptions] — относительный, переживает смену разрешения. */
    var videoBitrateAuto by mutableStateOf(true)
        private set
    var videoBitrateIdx by mutableStateOf(1)
        private set

    /** Три варианта битрейта (бит/с) под разрешение/fps: ниже · рекоменд. · выше. */
    fun bitrateOptions(w: Int, h: Int, fps: Int): List<Int> {
        // Три РАЗНЫХ варианта: ниже · рекомендация · выше. Раньше всё округлялось до
        // целого мегабита, а рекомендация не бывает меньше 1 Мбит/с, — на 480p все три
        // чипа были «1 Мбит/с».
        val step = VIDEO_BITRATE_STEP
        fun snap(x: Int) = ((x + step / 2) / step * step).coerceAtLeast(step)
        val mid = snap(autoBitrate(w, h, fps))
        val low = snap(mid * 6 / 10).coerceAtMost(mid - step).coerceAtLeast(step)
        val high = snap(mid * 3 / 2).coerceAtLeast(mid + step)
        return listOf(low, mid, high)
    }

    /** Изменить видео по умолчанию (что не передано — остаётся) и сохранить. */
    fun setVideoDefaults(
        front: Boolean = cameraFront,
        w: Int = videoQuality.width,
        h: Int = videoQuality.height,
        fps: Int = videoQuality.fps,
        bitrateAuto: Boolean = videoBitrateAuto,
        bitrateIdx: Int = videoBitrateIdx,
    ) {
        cameraFront = front
        videoBitrateAuto = bitrateAuto
        videoBitrateIdx = bitrateIdx.coerceIn(0, 2)
        val bps = bitrateOptions(w, h, fps)[if (bitrateAuto) 1 else videoBitrateIdx]
        videoQuality = VideoQuality("${h}p", w, h, fps, bps)
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putBoolean("video_front", front)
            ?.putInt("video_w", w)
            ?.putInt("video_h", h)
            ?.putInt("video_fps", fps)
            ?.putBoolean("video_br_auto", videoBitrateAuto)
            ?.putInt("video_br_idx", videoBitrateIdx)
            ?.apply()
    }

    /** Тех-инфо оверлей в звонке (пинг/разрешение/fps/битрейт на тайлах). Тумблер в ⚙️. */
    var showTechInfo by mutableStateOf(false)
        private set

    fun toggleTechInfo() {
        showTechInfo = !showTechInfo
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("tech_info", showTechInfo)?.apply()
    }

    /** Предупреждать о разросшейся очереди недоставленного (вкл по умолчанию).
     *  Сообщения при этом НЕ удаляются — это только уведомление. Тумблер в ⚙️. */
    var warnOutboxGrowth by mutableStateOf(true)
        private set

    fun toggleOutboxWarning() {
        warnOutboxGrowth = !warnOutboxGrowth
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("warn_outbox", warnOutboxGrowth)?.apply()
    }

    /** Показать подсказку про входящий на весь экран ([checkFullScreenCalls]). */
    var fullScreenHint by mutableStateOf(false)
        private set

    /**
     * Android 14+ даёт полноэкранный интент не всем: без него входящий на заблокированном экране
     * — только уведомлением. Подсказка — один раз, при первом таком звонке.
     */
    private fun checkFullScreenCalls(ctx: android.content.Context) {
        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return
        val prefs = ctx.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
        if (prefs.getBoolean("fsi_hint_seen", false)) return
        val nm = ctx.getSystemService(android.app.NotificationManager::class.java)
        if (!nm.canUseFullScreenIntent()) fullScreenHint = true
    }

    fun dismissFullScreenHint() {
        fullScreenHint = false
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("fsi_hint_seen", true)?.apply()
    }

    /** Присланное другой программой («Поделиться», 7d), пока не отправлено. */
    var shareDraft by mutableStateOf<ShareDraft?>(null)

    /** Что открыть по ярлыку на значке или ярлыку чата; экран забирает и сбрасывает. */
    var launchTarget by mutableStateOf<LaunchTarget?>(null)

    /**
     * Последние чаты в системном меню «Поделиться» (выкл по умолчанию): выключенным имена и
     * фото чатов системе не передаются. Выключение снимает уже опубликованные ярлыки.
     */
    var shareChatsInMenu by mutableStateOf(false)
        private set

    fun setShareChats(on: Boolean) {
        shareChatsInMenu = on
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("share_chats", on)?.apply()
        // Значки чатов рисуются в битмапы — не на главном потоке.
        appContext?.let { c -> reads.execute { ShareShortcuts.sync(c, this) } }
    }

    /** Уведомлять о флуде на адрес (вкл по умолчанию). Сам учёт потока и «Сеть» — всегда. */
    var notifyFlood by mutableStateOf(true)
        private set

    /** «Звуки в чате»: капля на приход в открытый чат и на отправку ([ChatSounds]). */
    var chatSounds by mutableStateOf(true)
        private set

    fun setChatSoundsOn(on: Boolean) {
        chatSounds = on
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("chat_sounds", on)?.apply()
    }

    /** Капля отправки — на нажатие «отправить», а не на эхо из движка; ответ из шторки — без неё. */
    private fun chimeSent() {
        if (!foreground) return
        appContext?.let { ChatSounds.play(it, ChatSounds.Kind.Out, chatSounds) }
    }

    /** «Запросы и приглашения» в шторке ([RequestNotifier]). */
    var notifyRequests by mutableStateOf(true)
        private set

    fun setNotifyRequestsOn(on: Boolean) {
        notifyRequests = on
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("notif_requests", on)?.apply()
    }

    /** «Текст в уведомлении»: выключено — в шторке «Новое сообщение» ([MessageNotifier]). */
    var notifyText by mutableStateOf(true)
        private set

    fun setNotifyTextOn(on: Boolean) {
        notifyText = on
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("notif_text", on)?.apply()
    }

    /** Состояние «связи» для шторки ([ConnState]). Снимок-карты читаются с тикера безопасно. */
    private fun connState(nowOffline: Boolean, peers: Int): ConnState {
        val dm = unread.filterValues { it > 0 }
        val grp = groupUnread.filterValues { it > 0 }
        return ConnState(
            offline = nowOffline,
            searching = peers == 0,
            messages = dm.values.sum() + grp.values.sum(),
            chats = dm.size + grp.size,
            mention = groupMentioned.isNotEmpty(),
            requests = contactRequests.size,
            invites = groupInvites.size,
        )
    }

    /** Перестроить «связь» сейчас, не дожидаясь тикера (в фоне он спит 15 с). */
    private fun refreshConn() {
        appContext?.let { ConnService.refresh(it, connState(offline, netPeers)) }
    }

    /** «Прочитать всё» в «связи»: каждый чат с непрочитанным — как «Прочитано» в его уведомлении. */
    fun readAll() {
        val ctx = appContext
        val chats = unread.filterValues { it > 0 }.keys.map { ChatTarget(it, group = false) } +
            groupUnread.filterValues { it > 0 }.keys.map { ChatTarget(it, group = true) }
        chats.forEach { c ->
            readFromNotification(c)
            ctx?.let { MessageNotifier.cancel(it, c) }
        }
        refreshConn()
    }

    /** «Прочитано» в шторке: как открыть чат — счётчик в ноль, отметка собеседнику. */
    fun readFromNotification(chat: ChatTarget) {
        if (chat.group) {
            groupUnread[chat.id] = 0
            groupMentioned.remove(chat.id)
        } else {
            unread[chat.id] = 0
            ops.execute { Native.markRead(sessionPtr, chat.id) }
        }
        refreshConn()
    }

    /** «Ответить» в шторке: обычным сообщением, не трогая ответ, начатый в открытом чате. */
    fun replyFromNotification(chat: ChatTarget, text: String) {
        val keep = replyTarget
        replyTarget = null
        send(chat.id, text, group = chat.group)
        replyTarget = keep
        val title = if (chat.group) groups.firstOrNull { it.gid == chat.id }?.name.orEmpty() else peerName(chat.id)
        appContext?.let { MessageNotifier.appendMine(it, chat, title, text.trimEnd()) }
    }

    /** Входящее — в шторку, если правило [MessageNotifier.shouldNotify] пускает. */
    private fun notifyIncoming(chat: ChatTarget, m: Message, onScreen: Boolean, sent: Long) {
        val ctx = appContext ?: return
        val mode = notifyMode(chat.id, chat.group)
        val mentionsMe = chat.group && m.mentions.any { it.key == me?.pubkey }
        val now = System.currentTimeMillis()
        // Строка звонка обновляется по ходу (звонит → отвечен/пропущен) — по ней не уведомляем;
        // пропущенный поднимает [notifyMissedCall], когда звонящий сбросил без ответа.
        if (!MessageNotifier.shouldNotify(m.mine, m.system, m.isCall, false, onScreen && foreground, mode, mentionsMe, sent, now)) return
        val title = if (chat.group) groups.firstOrNull { it.gid == chat.id }?.name.orEmpty() else peerName(chat.id)
        val sender = contacts.firstOrNull { it.key == m.sender }
        val line = MessageNotifier.Line(
            id = m.id,
            sender = m.sender,
            name = peerName(m.sender),
            text = notificationText(ctx, m),
            time = now,
            avatar = sender?.avatarId?.let { contactAvatars[it] },
        )
        MessageNotifier.post(ctx, chat, title, mention = mentionsMe, line = line)
    }

    /**
     * Пропущенный звонок — строкой в разговор чата с «Перезвонить» (п. 7). Режим чата его не
     * глушит; лента этого чата на экране — без шторки (звонок и так был виден).
     */
    private fun notifyMissedCall(peer: String, video: Boolean) {
        val ctx = appContext ?: return
        val chat = ChatTarget(peer, group = false)
        val now = System.currentTimeMillis()
        val onScreen = viewing == chat && foreground
        if (!MessageNotifier.shouldNotify(false, false, true, true, onScreen, notifyMode(peer, false), false, 0, now)) return
        val line = MessageNotifier.Line(
            id = now,
            sender = peer,
            name = peerName(peer),
            text = ctx.getString(if (video) R.string.chat_call_missed_video else R.string.notif_missed_audio),
            time = now,
            avatar = contacts.firstOrNull { it.key == peer }?.avatarId?.let { contactAvatars[it] },
            call = true,
        )
        MessageNotifier.post(ctx, chat, peerName(peer), mention = false, line = line)
    }

    /**
     * Комната группы ожила ([RoomWatch]) — строкой в разговор группы с «Присоединиться» (п. 7):
     * только при режиме группы «все»; лента группы на экране — без шторки (комнату и так видно).
     */
    private fun notifyRoomAlive(gid: String, inside: List<RoomMember>) {
        val ctx = appContext ?: return
        val first = inside.firstOrNull() ?: return
        val chat = ChatTarget(gid, group = true)
        if (notifyMode(gid, true) != NotifyMode.All || (viewing == chat && foreground)) return
        val now = System.currentTimeMillis()
        val line = MessageNotifier.Line(
            id = now,
            sender = first.peer,
            name = peerName(first.peer),
            text = ctx.getString(R.string.notif_room_line),
            time = now,
            avatar = contacts.firstOrNull { it.key == first.peer }?.avatarId?.let { contactAvatars[it] },
            call = true,
        )
        val title = groups.firstOrNull { it.gid == gid }?.name ?: return
        MessageNotifier.post(ctx, chat, title, mention = false, line = line)
    }

    /**
     * Реакция на моё сообщение (beta.md 7f) — строкой «Боб: 👍 к «начало»» в разговор чата, по
     * тому же правилу, что сообщение: режим чата (реакция — не упоминание), лента на экране — без
     * шторки (там капсула «❤»).
     */
    private fun notifyReaction(chat: ChatTarget, news: EngineEvent.ReactNewsDto) {
        val ctx = appContext ?: return
        val now = System.currentTimeMillis()
        val onScreen = viewing == chat && foreground
        if (!MessageNotifier.shouldNotify(false, false, false, false, onScreen, notifyMode(chat.id, chat.group), false, 0, now)) return
        val title = if (chat.group) groups.firstOrNull { it.gid == chat.id }?.name.orEmpty() else peerName(chat.id)
        val line = MessageNotifier.Line(
            id = now,
            sender = news.by,
            name = peerName(news.by),
            text = reactionText(ctx, news.emoji, news.quote, news.kind),
            time = now,
            avatar = contacts.firstOrNull { it.key == news.by }?.avatarId?.let { contactAvatars[it] },
        )
        MessageNotifier.post(ctx, chat, title, mention = false, line = line)
    }

    /** Текст строки шторки: подпись или текст, у вложения без подписи — его вид. */
    private fun notificationText(ctx: android.content.Context, m: Message): String =
        if (m.text.isNotBlank()) Markdown.plain(Mentions.plain(m.text, m.mentions, ::mentionName)) else previewText(ctx, "", m.kind)

    /** «👍 к «начало»» — одна строка для шторки и списка чатов (начало — до [QUOTE_MAX]). */
    fun reactionText(ctx: android.content.Context, emoji: String, quote: String, kind: String): String {
        val q = previewText(ctx, quote, kind).replace('\n', ' ')
            .let { if (it.length > QUOTE_MAX) it.take(QUOTE_MAX).trimEnd() + "…" else it }
        return ctx.getString(R.string.notif_reaction, emoji, q)
    }

    private fun previewText(ctx: android.content.Context, text: String, kind: String): String = when {
        text.isNotBlank() -> text
        kind == "image" -> ctx.getString(R.string.chats_preview_photo)
        kind == "video" -> ctx.getString(R.string.chats_preview_video)
        kind == "voice" -> ctx.getString(R.string.chats_preview_voice)
        else -> ctx.getString(R.string.chats_preview_file)
    }

    fun setFloodNotify(on: Boolean) {
        notifyFlood = on
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putBoolean("notif_flood", on)?.apply()
        if (!on) appContext?.let { FloodNotifier.cancelFlood(it) }
    }

    /** Сколько сообщений ждёт доставки и надо ли предупредить (из движка). */
    var outboxPending by mutableStateOf(0)
        internal set
    var outboxWarn by mutableStateOf(false)
        private set

    private fun refreshOutboxStats() {
        val s = decodeOrNull<OutboxStatsDto>(Native.outboxStats(sessionPtr)) ?: return
        outboxPending = s.pending
        outboxWarn = s.warn
    }

    // ── Хранилище (простой экран; нормальный дизайн — позже) ──────────────────
    var storageBlobs by mutableStateOf(0L)
        internal set
    var storageBlobBytes by mutableStateOf(0L)
        internal set
    var storageOutboxPending by mutableStateOf(0L)
        internal set

    /** Что занимает место, по типам вложения: kind → байты. */
    var storageByKind by mutableStateOf<List<Pair<String, Long>>>(emptyList())
        internal set

    /** По чатам, самый тяжёлый первым. */
    var storageChats by mutableStateOf<List<StorageChat>>(emptyList())
        internal set

    /** Автоудаление: общее правило и чаты со своим (5.4). */
    internal var autodel by mutableStateOf(AutodelStateDto())

    // ── Резервная копия ДАННЫХ (M4) ───────────────────────────────────────────
    // Не путать с копией КЛЮЧА ([revealSeedPhrase]): ключ возвращает личность, копия
    // данных — переписку.

    /** Когда сделана последняя копия данных (unix-секунды); 0 — не делалась.
     *  Локальный факт: сам файл мы после сохранения не видим (он у пользователя), а
     *  сказать «копии нет» после её создания — врать. */
    val dataBackupAt get() = backupFacts.dataBackupAt

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
        if (stats.error.isEmpty()) onMain { updateBackup { dataDone(it) } }
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
            // Своё имя и фото могли приехать из копии (шаг 7g).
            selfName = Native.selfName(sessionPtr)
            selfAvatar = Native.selfAvatar(sessionPtr)
        }
        return stats
    }

    /** Очередь по получателям: (ключ, сколько ждёт, байт, время самой старой записи). */
    var outboxByPeer by mutableStateOf<List<OutboxPeer>>(emptyList())
        internal set

    /**
     * Закреплённые сообщения по чатам (ключ собеседника или gid), свежие первыми.
     * Шапка чата показывает верхнее; закреп общий для всех участников, поэтому
     * список приходит из движка, а не собирается на клиенте.
     */
    val pinned = mutableStateMapOf<String, List<Message>>()

    /** Перечитать закреплённые для чата. */
    fun refreshPins(chat: String, group: Boolean) = ops.execute {
        pinned[chat] = decodeList<MessageDto>(Native.pinned(sessionPtr, chat, group))
            .map { it.toModel() }
    }

    /** Моя реакция на сообщение (null — не ставил). */
    fun myReaction(msg: Message): String? = me?.pubkey?.let { k -> msg.reactions.firstOrNull { k in it.who }?.emoji }

    /**
     * Реакция на сообщение: у человека одна — та же снимает, другая заменяет. Решает
     * клиент, движок хранит последнее слово; набор вернётся событием `reactions`.
     */
    fun react(chat: ChatTarget, msg: Message, emoji: String) {
        val next = if (myReaction(msg) == emoji) "" else emoji
        // Снятая реакция не считается: ряд — про то, что ставят, а не что убирают.
        if (next.isNotEmpty()) countReaction(next)
        ops.execute { Native.react(sessionPtr, chat.id, chat.group, msg.id, msg.sender, next) }
    }

    /** Закрепить/открепить выделенное сообщение. */
    fun setPinned(chat: String, group: Boolean, msg: Message, on: Boolean) = ops.execute {
        Native.pinMessage(sessionPtr, chat, group, msg.id, msg.sender, on)
        refreshPins(chat, group)
    }

    /** Находка поиска по переписке: где нашлось + само сообщение. */
    data class SearchHit(
        val chat: String,
        val group: Boolean,
        val id: Long,
        val sender: String,
        val text: String,
        val mine: Boolean,
        val time: Long,
        val row: Long,
    )

    /** Результаты поиска по сообщениям (свежие первыми). */
    val searchHits = mutableStateListOf<SearchHit>()

    /** Строка общего поиска: живёт в модели, потому что поле — в шапке, а находки — на панели. */
    var globalQuery by mutableStateOf("")

    /** Журнал звонков (свежие первыми). */
    val callLog = mutableStateListOf<CallEntry>()

    /** Наборы сверки по ключу контакта. Снапшот-состояние: первый запрос из UI ставит
     *  расчёт в [ops], сетка дорисуется, когда он придёт (инвариант 4: не JNI из композиции). */
    private val pairFingerprints = mutableStateMapOf<String, PairFingerprint>()

    /** Ключи, чей набор уже считается. НЕ снапшот-состояние: функцию зовёт композиция,
     *  а писать в состояние из композиции, пока его же пишет фоновый поток, —
     *  «Unsupported concurrent change during composition» и падение. */
    private val pairFingerprintsRequested = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

    /** Набор сверки с [key]: один на пару, считает движок. Пока не посчитан — null. */
    fun pairFingerprint(key: String): PairFingerprint? {
        pairFingerprints[key]?.let { return it }
        if (sessionPtr != 0L && pairFingerprintsRequested.add(key)) {
            ops.execute {
                val fp = runCatching {
                    val o = JSONObject(Native.pairFingerprint(sessionPtr, key))
                    val w = o.getJSONArray("words")
                    PairFingerprint(o.getString("glyphs"), List(w.length()) { w.getString(it) })
                }.getOrNull() ?: return@execute
                onMain { pairFingerprints[key] = fp }
            }
        }
        return null
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
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.edit()?.putString("folders", arr.toString())?.apply()
    }

    private fun loadFolders() {
        val raw = appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
            ?.getString("folders", null) ?: return
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return
        val loaded = mutableListOf<Folder>()
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            fun strSet(name: String): Set<String> {
                val a = o.optJSONArray(name) ?: return emptySet()
                return (0 until a.length()).mapNotNull { a.optString(it).takeIf { s -> s.isNotEmpty() } }.toSet()
            }
            loaded += Folder(
                id = o.optString("id"),
                name = o.optString("name"),
                icon = o.optString("icon"),
                types = strSet("types"),
                chats = strSet("chats"),
                excludeRead = o.optBoolean("exclude_read", false),
            )
        }
        folders.replaceWith(loaded)
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

    /** Сколько чатов попадает в папку [f] сейчас: живой счётчик редактора и подпись в списке. */
    fun folderCount(f: Folder): Int =
        contacts.count { folderHasDm(f, it.key) } + groups.count { folderHasGroup(f, it.gid) }

    /**
     * Папка в редакторе: в модели, потому что её счётчик — в шапке, а правка — на панели.
     * Пишется в [folders] при выходе из редактора ([commitFolderDraft]), кнопки «Сохранить» нет.
     */
    var folderDraft by mutableStateOf<Folder?>(null)

    /** Записать черновик: без названия папки нет — новая пропадает, у старой остаётся прежнее имя. */
    fun commitFolderDraft() {
        val d = folderDraft ?: return
        folderDraft = null
        val old = folderById(d.id)
        val name = d.name.trim().ifEmpty { old?.name ?: return }
        val f = d.copy(name = name)
        if (f != old) saveFolder(f)
    }

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
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putString("recent_emoji", recentEmoji.joinToString(" "))?.apply()
    }

    /** Сколько раз я ставил каждую реакцию: из этого собирается быстрый ряд ([ReactionUsage]). */
    private var reactionCounts by mutableStateOf<Map<String, Int>>(emptyMap())

    /** Быстрый ряд над пузырём: частые свои реакции, добранные рядом по умолчанию. */
    val quickReactions: List<String> get() = ReactionUsage.quick(reactionCounts)

    private fun countReaction(e: String) {
        reactionCounts = reactionCounts + (e to (reactionCounts[e] ?: 0) + 1)
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putString("reaction_counts", ReactionUsage.format(reactionCounts))?.apply()
    }

    private fun loadSettings() {
        val prefs = appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE) ?: return
        showTechInfo = prefs.getBoolean("tech_info", false)
        reactionCounts = ReactionUsage.parse(prefs.getString("reaction_counts", null))
        recentEmoji = prefs.getString("recent_emoji", "")?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
        warnOutboxGrowth = prefs.getBoolean("warn_outbox", true)
        shareChatsInMenu = prefs.getBoolean("share_chats", false)
        notifyFlood = prefs.getBoolean("notif_flood", true)
        chatSounds = prefs.getBoolean("chat_sounds", true)
        notifyText = prefs.getBoolean("notif_text", true)
        notifyRequests = prefs.getBoolean("notif_requests", true)
        mediaColumns = prefs.getInt("media_columns", 3).takeIf { it in MEDIA_COLUMNS } ?: 3
        val w = prefs.getInt("video_w", VideoQuality.default.width)
        val h = prefs.getInt("video_h", VideoQuality.default.height)
        val fps = prefs.getInt("video_fps", VideoQuality.default.fps)
        cameraFront = prefs.getBoolean("video_front", true)
        videoBitrateAuto = prefs.getBoolean("video_br_auto", true)
        videoBitrateIdx = prefs.getInt("video_br_idx", 1).coerceIn(0, 2)
        val bps = bitrateOptions(w, h, fps)[if (videoBitrateAuto) 1 else videoBitrateIdx]
        videoQuality = VideoQuality("${h}p", w, h, fps, bps)
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
    fun shareBitrateOptions(): List<Int> = bitrateOptions(shareResW, shareResH, shareFps)

    /** Применяемый битрейт: рекомендация (idx 1) при «Авто», иначе выбранный вариант. */
    fun shareBitrateBps(): Int = shareBitrateOptions()[if (shareBitrateAuto) 1 else shareBitrateIdx]

    fun openVideoShare() {
        shareScreenMode = false
        shareFront = cameraFront
        shareResW = videoQuality.width
        shareResH = videoQuality.height
        shareFps = videoQuality.fps
        shareBitrateAuto = videoBitrateAuto
        shareBitrateIdx = videoBitrateIdx
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
        // Выбор в звонке становится и новым значением по умолчанию — как в настройках.
        setVideoDefaults(shareFront, shareResW, shareResH, shareFps, shareBitrateAuto, shareBitrateIdx)
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

    /** Лист «Позвать в звонок» (групповая комната). В модели, а не в экране: кнопка
     *  живёт в шапке группового чата, а лист рисуется поверх всего приложения. */
    var showRingSheet by mutableStateOf(false)

    /** Группа, чат которой открыть: позвали в её комнату, и мы вошли. Полоса «В
     *  звонке» и кнопка выхода живут в чате группы — без перехода человек оставался
     *  на списке и не видел, что он в звонке. Гасит MainActivity, исполнив. */
    var openGroupRequest by mutableStateOf<String?>(null)

    /** Вывод звонка: "builtin" (встроенный/разговорный), "speaker" (громкая), "bluetooth". */
    var audioOut by mutableStateOf("builtin")
        private set

    /** Вход (микрофон): "builtin", "headset" (проводная гарнитура), "bluetooth". */
    var audioIn by mutableStateOf("builtin")
        private set

    /** Аппаратная обработка микрофона (AEC/NS/AGC). */
    var audioFx by mutableStateOf(AudioFx())
        private set

    /** Качество звука: Авто / Голос / Аудио ([AudioMode]). */
    var audioMode by mutableStateOf(AudioMode.AUTO)
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

    fun pickAudioMode(mode: AudioMode) {
        audioMode = mode
        saveAudioPrefs() // битрейт — на лету через applyAudioSettings, режим Opus — со следующего звонка
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
        appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)?.edit()
            ?.putString("audio_out", audioOut)
            ?.putString("audio_in", audioIn)
            ?.putBoolean("fx_ns", audioFx.ns)
            ?.putBoolean("fx_aec", audioFx.aec)
            ?.putBoolean("fx_agc", audioFx.agc)
            ?.putString("aq_mode", audioMode.name)
            ?.apply()
    }

    private fun loadAudioPrefs() {
        val p = appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE) ?: return
        audioOut = p.getString("audio_out", "builtin") ?: "builtin"
        audioIn = p.getString("audio_in", "builtin") ?: "builtin"
        audioFx = AudioFx(p.getBoolean("fx_ns", true), p.getBoolean("fx_aec", true), p.getBoolean("fx_agc", true))
        audioMode = AudioMode.entries.firstOrNull { it.name == p.getString("aq_mode", null) } ?: AudioMode.AUTO
    }

    var appContext: android.content.Context? = null

    /** Приложение на переднем плане? Ставит MainActivity (onStart/onStop). В фоне
     *  poll/ticker троттлятся — меньше пробуждений CPU при удержанном wakelock. */
    @Volatile var foreground = true
        set(v) {
            field = v
            // Своим — «в сети» или уже нет (если человек это не скрыл; решает движок).
            ops.execute { Native.setAppForeground(sessionPtr, v) }
            // Вернулись к ленте чата — она снова на экране, значит прочитана, и шторка о нём не нужна.
            if (v) {
                viewing?.let { c ->
                    appContext?.let { MessageNotifier.cancel(it, c) }
                    // Пришедшее в него, пока приложение было свёрнуто, теперь на экране.
                    if (c.group) {
                        groupUnread[c.id] = 0
                        groupMentioned.remove(c.id)
                    } else {
                        unread[c.id] = 0
                    }
                }
            }
            if (v) viewing?.takeUnless { it.group }?.let { c -> ops.execute { Native.markRead(sessionPtr, c.id) } }
        }

    /** Отчёты о прочтении и набор (2.10): переключатели «Приватности». */
    var privacy by mutableStateOf(PrivacyDto())
        private set

    /** Собеседник печатает: ключ → до какого момента (мс) показывать. Последнее
     *  «печатаю» живёт [TYPING_SHOWN_MS]; отправитель повторяет его раз в 3 с. */
    val typingUntil = mutableStateMapOf<String, Long>()
    var incomingCall by mutableStateOf<IncomingRoomCall?>(null)
    var dmCallPeer by mutableStateOf<String?>(null)
    var videoActive by mutableStateOf(false)

    @Volatile private var roomAudio: CallAudio? = null

    /**
     * «Положить трубку» не стоит в очереди. Выход разбирает звонок своим потоком,
     * а не через [ops]: в [ops] попадает и работа, вызванная событиями из сети, и
     * собеседник не должен суметь задержать остановку нашего микрофона (класс Signal
     * RingRTC CVE-2026-113380). Но старт медиа живёт в [ops], и выход мог бы его
     * обогнать — тогда микрофон включился бы ПОСЛЕ выхода. Поэтому у звонка есть
     * поколение ([callGen], растёт на каждом выходе), а старт и разбор идут под одним
     * замком ([callLock]): разбор либо застанет запущенное и остановит его, либо
     * отложенный старт увидит новое поколение и не начнётся.
     */
    private val callGen = java.util.concurrent.atomic.AtomicInteger()
    private val callLock = Any()

    /** Выполнить старт звонка в [ops], если звонок за это время не завершили.
     *  [after] — разбор ПРЕДЫДУЩЕГО звонка (переключение): ждём его до замка, иначе
     *  старт мог занять замок первым, а разбор потом погасил бы уже новое аудио. */
    private fun startCall(after: Thread? = null, block: () -> Unit) {
        val gen = callGen.get()
        ops.execute {
            after?.join()
            synchronized(callLock) { if (callGen.get() == gen) block() }
        }
    }

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
    // `internal`, а не `private`: поведение модели разнесено по зонам-файлам
    // (заметки, поиск, хранилище), и всем им нужен один и тот же поток-актор.
    // Заводить второй значило бы получить два порядка операций над одной сессией.
    internal val ops = Executors.newSingleThreadExecutor { Thread(it, "native-ops").apply { isDaemon = true } }

    // Долгое — мимо [ops], иначе пока шифруется видео на гигабайт, не открываются
    // чаты и не уходит текст (R-051). Один поток: вложения уходят в порядке отправки.
    internal val heavy = Executors.newSingleThreadExecutor { Thread(it, "native-heavy").apply { isDaemon = true } }

    // Чтения: байты картинок для ленты и поиск. Отдельно от [heavy], чтобы прокрутка
    // не ждала отправки файла, и от [ops], чтобы команды не ждали диска.
    internal val reads = Executors.newSingleThreadExecutor { Thread(it, "native-reads").apply { isDaemon = true } }

    @Volatile private var running = false
    private var pollThread: Thread? = null
    private var tickerThread: Thread? = null
    private var netCallback: ConnectivityManager.NetworkCallback? = null

    @Volatile private var started = false

    private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())

    /** Запись снапшот-состояния с фонового цикла — на главном потоке (см. тикер). */
    private fun onMain(write: () -> Unit) {
        mainHandler.post(write)
    }

    /** То же для зон модели в других файлах (ModelFeed и т.п.). */
    internal fun runOnMain(write: () -> Unit) = onMain(write)

    // Офлайн-очередь целиком в движке: недоставленное лежит в его зашифрованной
    // базе и досылается им же. Здесь — только отрицательные id для того, что движок
    // не смог даже поставить в очередь.
    @Volatile private var localMsgId = -1L

    /** Инициализация ядра. Звать с фонового потока (см. MainActivity).
     *  Идемпотентно: при пересоздании Activity повторно НЕ инициализирует. */
    /**
     * Инициализация ядра. `password` нужен на уровнях at-rest PASSWORD/BOTH.
     * Возвращает false, если пароль не подошёл, — вызывающий показывает ошибку, а
     * не стартует с мусорным ключом (иначе база выглядела бы повреждённой).
     */
    /** [key] — ключ хранилища, уже развёрнутый отпечатком: пароль тогда не нужен. */
    // Звать могут двое сразу: экран и служба связи, поднятая загрузкой телефона (7h).
    @Synchronized
    fun start(dataDir: String, password: String? = null, key: ByteArray? = null): Boolean {
        if (started) return true
        val vaultKey = key ?: runCatching { VaultKeyManager.getOrCreate(File(dataDir), password) }
            .getOrElse {
                // С паролем сбой значит «неверный пароль» — это покажет экран ввода.
                // Без пароля ключ не развернулся: Keystore потерял свой ключ (сменили
                // блокировку экрана, обновили прошивку). Тупик «не запускается» —
                // хуже любого выбора, поэтому показываем экран восстановления.
                if (password == null) recovery = Recovery.KEY_LOST
                return false
            }
        started = true
        loadSettings()
        loadAudioPrefs()
        val started = decodeOrNull<StartResultDto>(Native.start(dataDir, vaultKey))
        if (started == null || started.error != null || started.sessionPtr == 0L) {
            this.started = false
            // Данные зашифрованы другим ключом: движок их не стёр, решает человек.
            if (started?.error == START_VAULT_MISMATCH) {
                recoveryKey = vaultKey
                recovery = Recovery.DATA_UNREADABLE
            } else {
                error = started?.error ?: "engine start failed"
            }
            return false
        }
        sessionPtr = started.sessionPtr
        transportEpoch = Native.transportEpoch(sessionPtr)
        val prefs = appContext
            ?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
        prefs?.let { p -> BackupFacts.load(p, System.currentTimeMillis() / 1000).let { onMain { backupFacts = it } } }
        refreshContactsNow()
        refreshRequestsNow()
        refreshBlockedNow()
        refreshGroupsNow()
        refreshGroupInvitesNow()
        refreshChatSummariesNow() // список чатов с превью — сразу, до первого события
        loadWanted() // докачки, заказанные до перезапуска
        loadFolders()
        selfAvatar = Native.selfAvatar(sessionPtr) // M2: свой аватар (уже на этом потоке)
        selfName = Native.selfName(sessionPtr) // M2: своё имя из движка
        privacy = decodeOrNull<PrivacyDto>(Native.privacy(sessionPtr)) ?: PrivacyDto()
        Native.setAppForeground(sessionPtr, foreground) // смены до старта сессии движок не видел
        contactCode = Native.myContactCode(sessionPtr)
        // Расшифрованные копии вложений, оставшиеся от прошлого запуска (просмотр,
        // «открыть», «поделиться»): держать их дольше нужного незачем.
        appContext?.cacheDir?.let { c -> listOf("viewer", "shared").forEach { File(c, it).deleteRecursively() } }
        // `me` — гейт главного экрана (MainActivity), поэтому ПОСЛЕДНИМ: экран начинал
        // строиться, пока выше ещё грузились имя, контакты и группы. Записи с этого
        // потока посреди первой (после установки — медленной) композиции терялись,
        // и шапка жила с пустым именем и «0 пиров» до перезахода.
        me = Me(started.pubkey, started.address, started.fingerprint)
        startLoops()
        registerNetworkCallback()
        appContext?.let {
            BootReceiver.dismissLocked(it)
            ChatSounds.init(it)
        }
        return true
    }

    /** Снимок состояния для «Сохранить лог» (7n) — одни числа; null — движок не поднят. */
    fun diagnosticsJson(): String? = if (started && sessionPtr != 0L) Native.diagnostics(sessionPtr) else null

    // Мгновенный реконнект при восстановлении сети (до срабатывания тикера).
    private fun registerNetworkCallback() {
        val ctx = appContext ?: return
        val cm = ctx.getSystemService(ConnectivityManager::class.java) ?: return
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                val ptr = sessionPtr
                if (ptr != 0L) {
                    android.util.Log.i("net", "Network available — forcing peer reconnect")
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
                // Сводка чатов пересчитывается РАЗ на пачку событий, а не на каждое:
                // при разгребании офлайн-очереди событий приходят сотни, а список
                // чатов всё равно перерисуется один раз.
                if (summariesDirty) {
                    summariesDirty = false
                    runCatching { refreshChatSummariesNow() }
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
        // Тикер опрашивает движок на своём потоке, а в состояние пишет через [onMain].
        // Запись отсюда, попавшая на первую композицию экрана, терялась: экран читал
        // старое значение, а уведомление об изменении уходило мимо ещё не собранной
        // композиции. Тикер же пишет одно и то же — «0 пиров» висело до перезахода.
        // На главном потоке запись не может пересечься с композицией: та идёт там же.
        tickerThread = Thread {
            var tick = 0L
            var zeroPeerTicks = 0 // счётчик тиков без пиров
            val roomWatch = RoomWatch(System.currentTimeMillis())
            while (running) {
                runCatching {
                    val p = Native.netPeers(sessionPtr)
                    val t = Native.netTree(sessionPtr)
                    val presence = contacts.map { it.key }.associateWith { Native.isOnline(sessionPtr, it) }
                    // Флуд и офлайн — дёшево и нужно даже в фоне: пользователь
                    // платит за чужой поток прямо сейчас.
                    val nowOffline = Native.isOffline(sessionPtr)
                    onMain {
                        netPeers = p
                        presence.forEach { (key, on) -> online[key] = on }
                        offline = nowOffline
                        if (transfers.isNotEmpty()) refreshTransfers() // отправитель ушёл / вернулся
                    }
                    appContext?.let { ConnService.refresh(it, connState(nowOffline, p)) }
                    val flood = decodeList<FloodPeerDto>(Native.floodPeers(sessionPtr))
                        .map { FloodPeer(it.peer, it.dropped, it.bytes, it.pps, it.bps, it.blocked) }
                    if (floodPeers != flood) {
                        onMain { floodPeers.replaceWith(flood) }
                    }
                    // Уведомление — ОДИН раз на эпизод: иначе оно само станет вторым
                    // флудом. Эпизод считается законченным, когда поток стих.
                    val top = flood.maxByOrNull { it.pps }
                    if (top != null && top.pps >= FLOOD_NOTIFY_PPS) {
                        if (!floodNotified) {
                            floodNotified = true
                            appContext?.takeIf { notifyFlood }?.let { ctx ->
                                val who = contacts.firstOrNull { it.key == top.peer }?.displayName
                                    ?: (top.peer.take(8) + "…")
                                FloodNotifier.showFlood(ctx, who, top.pps.toInt())
                            }
                        }
                    } else if (flood.none { it.bps > 0 }) {
                        floodNotified = false
                    }
                    // Ход смены адреса: «уведомлено N из M» считает движок.
                    if (rotation != null) {
                        decodeOrNull<RotationProgressDto>(Native.rotationProgress(sessionPtr))?.let { p ->
                            onMain { rotation = rotation?.copy(told = p.told) }
                        }
                    }
                    if (benchRecv) {
                        val mbps = decodeOrNull<BenchDto>(Native.benchRecvStats(sessionPtr))?.mbps ?: 0.0
                        onMain { benchRecvMbps = mbps }
                    }
                    // Очередь недоставленного: раз в ~10 тиков хватает (ничего не
                    // теряется, это только повод показать предупреждение).
                    if (tick % 10 == 0L) refreshOutboxStats()
                    // Снапшот стата сети — только на переднем плане (экран «Сеть»).
                    if (foreground) {
                        decodeOrNull<NetStatsDto>(Native.netStats(sessionPtr))?.let { n ->
                            val stats = NetStats(
                                n.peerCount,
                                n.tree,
                                n.pingMs,
                                n.rxRate,
                                n.txRate,
                                n.peers.map {
                                    NetPeer(it.key, it.inbound, it.up, it.latencyMs, it.rxRate, it.txRate, it.uptimeS, it.addr, it.uri)
                                },
                            )
                            onMain { netStats = stats }
                        }
                    }
                    // Присутствие в комнате: кто сейчас внутри (по свежим маячкам).
                    currentRoom?.let { gid ->
                        val list = decodeList<RoomMemberDto>(Native.roomParticipants(sessionPtr, gid))
                            .map { RoomMember(it.peer, it.audio, it.video) }
                        // Только при реальном изменении состава/флагов — иначе каждый тик
                        // (2с в звонке) форсит рекомпозицию всего экрана → рывок видео.
                        if (roomMembers != list) {
                            onMain {
                                if (currentRoom == gid && roomMembers.toList() != list) {
                                    roomMembers.replaceWith(list)
                                    appContext?.let { CallService.refresh(it) }
                                }
                            }
                        }
                    }
                    // Присутствие в группах, к которым мы НЕ подключены:
                    // показывает кто уже в комнате до того как мы вошли.
                    val nowMs = System.currentTimeMillis()
                    groups.forEach { g ->
                        if (g.gid == currentRoom) {
                            roomWatch.observe(g.gid, true, nowMs) // своя — живая, не «ожила»
                            return@forEach
                        }
                        val list = decodeList<RoomMemberDto>(Native.roomPresencePeers(sessionPtr, g.gid))
                            .map { RoomMember(it.peer, it.audio, it.video) }
                        if (groupPresence[g.gid] != list) onMain { groupPresence[g.gid] = list }
                        if (roomWatch.observe(g.gid, list.isNotEmpty(), nowMs)) onMain { notifyRoomAlive(g.gid, list) }
                    }
                    // Диагностика + авто-реконнект меша.
                    // peers=0 два тика подряд (~6с) → Yggdrasil потерял все пиры.
                    // Принудительно re-add чтобы обойти внутренний backoff.
                    if (p == 0) {
                        zeroPeerTicks++
                        if (zeroPeerTicks >= 2) {
                            android.util.Log.w("net", "peers=0 for $zeroPeerTicks ticks — forcing peer reconnect")
                            Native.reconnectPeers(sessionPtr)
                            zeroPeerTicks = 0
                        }
                    } else {
                        zeroPeerTicks = 0
                    }
                    android.util.Log.i("net", "tick=$tick peers=$p tree=$t incoming=${incomingCall != null} room=${currentRoom != null}")
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
        contacts.replaceWith(list)
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
        // Выделение, ответ и правка теперь общие на все чаты, а принадлежат они
        // ОТКРЫТОМУ: без сброса выбранное в одном чате оказалось бы «выбранным» и
        // в следующем, а действие ушло бы не туда.
        leaveChat()
        currentChat = key
        unread[key] = 0
        ops.execute { loadHistory(key) }
        loadUnseenReactions(ChatTarget(key, group = false))
    }

    private fun loadHistory(key: String) {
        val list = decodeList<MessageDto>(Native.history(sessionPtr, key))
            .map { it.toModel() }
            .toMutableList()
        messages[chatKey(key, group = false)] = list.toMutableStateList()
        atChatStart -= key // хвост перечитан — выше снова может что-то быть
        detached.remove(chatKey(key, group = false))
        // Переписка на экране — прочитана (отметка уйдёт, только если отчёты включены).
        if (viewing == ChatTarget(key, group = false) && foreground) Native.markRead(sessionPtr, key)
        // Байты вложений НЕ читаем заранее: `blob()` возьмёт их при показе, то есть
        // только для видимых пузырей. Раньше вход в чат тянул с диска всю страницу,
        // включая вложения, до которых человек не долистает.
    }

    private fun log(key: String): SnapshotStateList<Message> =
        messages.getOrPut(chatKey(key, group = false)) { mutableListOf<Message>().toMutableStateList() }

    /** Чаты, где мы уже дошли до начала переписки (просить выше нечего). */
    internal val atChatStart = mutableSetOf<String>()

    /** Чаты/группы с идущей догрузкой — чтобы прокрутка не запускала её пачкой. */
    internal val loadingOlder = mutableSetOf<String>()

    /**
     * Догрузить страницу истории ВЫШЕ показанной. Открытие чата отдаёт только хвост
     * (движок не поднимает переписку целиком), остальное приезжает по мере прокрутки.
     * Вид чата — параметр: страницы приходят из разных вызовов движка, но условия
     * («уже в начале», «загрузка идёт») и место вставки общие.
     *
     * [done] получает, сколько сообщений добавлено — вызывающему это нужно, чтобы
     * удержать позицию списка: вставка сверху иначе выглядит как прыжок.
     */
    fun loadOlder(key: String, done: (Int) -> Unit, group: Boolean = false) {
        if (group) {
            loadOlderGroup(key, done)
            return
        }
        val list = messages[chatKey(key, group = false)]
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
                val fresh = withoutShown(list, page)
                if (fresh.isNotEmpty()) list.addAll(0, fresh)
                done(fresh.size)
            } finally {
                loadingOlder -= key
            }
        }
    }

    /**
     * Страница истории без того, что уже в ленте. Своё только что отправленное лежит
     * в ленте без номера строки, и курсор страницы берётся с ПЕРВОЙ строки с номером
     * — а это может быть пришедший позже ответ. Тогда страница «старше ответа»
     * приносит наше же сообщение второй раз, с тем же (id, автор): LazyColumn падал
     * на повторе ключа, а в ленте висел бы дубль.
     */
    private fun withoutShown(list: List<Message>, page: List<Message>): List<Message> {
        val shown = list.mapTo(HashSet()) { it.id to it.sender }
        return page.filter { (it.id to it.sender) !in shown }
    }

    /** То же для группы: путь общий, отличается только источник страницы. */
    private fun loadOlderGroup(gid: String, done: (Int) -> Unit) {
        val list = messages[chatKey(gid, group = true)]
        val top = list?.firstOrNull { it.row != 0L }
        val mark = "g:$gid"
        if (list == null || top == null || mark in atChatStart || !loadingOlder.add(mark)) {
            done(0)
            return
        }
        ops.execute {
            try {
                val page = decodeList<MessageDto>(
                    Native.groupHistoryPage(sessionPtr, gid, top.time, top.row),
                ).map { it.toModel() }
                if (page.isEmpty()) atChatStart += mark
                val fresh = withoutShown(list, page)
                if (fresh.isNotEmpty()) list.addAll(0, fresh)
                done(fresh.size)
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

    /**
     * Сообщение, которое сейчас правят (null — обычный ввод). Поле ввода при этом
     * показывает старый текст: правка — это исправление, а не отправка заново.
     */
    var editTarget by mutableStateOf<Message?>(null)

    /** То же для группового сообщения. */

    /** Отправить правку своего сообщения (в группе — вместе с упоминаниями). */
    fun applyEdit(chat: String, group: Boolean, id: Long, text: String, mentions: List<Mention> = emptyList()) =
        ops.execute {
            if (text.isNotBlank()) Native.editMessage(sessionPtr, chat, group, id, text.trimEnd(), mentionsJson(mentions))
        }

    private fun mentionsJson(m: List<Mention>): String = Mentions.toJson(m)

    /** Имя для «@» в ленте: своё — моё имя, чужое — как этот человек записан у меня. */
    fun mentionName(key: String): String =
        if (key == me?.pubkey) selfName.ifBlank { appContext?.getString(R.string.common_you).orEmpty() } else peerName(key)

    /** Текст сообщения строкой, как его видно в ленте: «@» с моими именами, а не с
     *  метками автора. Всё, что показывает сообщение вне пузыря, берёт текст отсюда. */
    fun plainText(m: Message): String = Mentions.plain(m.text, m.mentions, ::mentionName)

    /** То же для показа вне пузыря (цитата, закреп, превью): без значков разметки, скрытое —
     *  точками. Копирование берёт [plainText] — со значками, чтобы разметка не терялась. */
    fun shownText(m: Message): String = Markdown.plain(plainText(m))

    /** Группы, где меня упомянули в непрочитанном. Как и счётчик, живёт в памяти. */
    val groupMentioned = mutableStateMapOf<String, Boolean>()

    internal fun wireDmRefs(refs: Collection<MsgRef>): String =
        dmRefsJson(refs, me?.pubkey.orEmpty())

    private fun wireGroupRefs(refs: Collection<MsgRef>): String = groupRefsJson(refs)

    /** Своё ли это сообщение: сравнение автора с собственным ключом. */
    fun isMine(ref: MsgRef): Boolean = ref.sender == me?.pubkey

    /** Уже отозвано «у всех» — отзывать нечего, остаётся «удалить у себя». */
    fun isRetracted(key: String, group: Boolean, ref: MsgRef): Boolean =
        messages[chatKey(key, group)].orEmpty().any { it.id == ref.id && it.sender == ref.sender && it.deleted }

    /** Начать пересылку выделенного в личном чате или «Сохранённом». */
    fun beginForward(chatKey: String, group: Boolean = false) {
        val refs = selection.map { it.id to it.sender }
        if (refs.isEmpty()) return
        forwarding = ChatTarget(chatKey, group) to refs
        clearSelection()
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
            val json = wireGroupRefs(refs.map { MsgRef(it.first, it.second) })
            Native.forwardMessages(sessionPtr, src.id, src.group, json, dst.id, dst.group, hide)
            // Пересланное — последнее в целевом чате, как своё отправленное (п. 58): движок
            // о своей отправке событий не шлёт, а в «Сохранённое» не придёт и подтверждение.
            markSummariesDirty()
            // Целевой чат может быть открыт прямо сейчас — перечитываем его хвост.
            if (dst.group && currentGroup == dst.id) {
                loadGroupHistory(dst.id)
            } else if (!dst.group && currentChat == dst.id) {
                loadHistory(dst.id)
            }
        }
    }

    /** Одно сообщение — в «Сохранённое»: та же пересылка себе, без выбора адресата. */
    fun saveToSaved(src: ChatTarget, m: Message) {
        val mine = me?.pubkey ?: return
        forwarding = src to listOf(m.id to m.sender)
        forwardHideAuthor = false
        forwardTo(ChatTarget(mine, group = false))
    }

    /** hex автора цитируемого сообщения: «моё» — это я, иначе собеседник. */
    private fun replyAuthor(peerKey: String, reply: Message?): String = when {
        reply == null -> ""
        reply.mine -> me?.pubkey.orEmpty()
        else -> peerKey
    }

    /**
     * Отправить текст в чат. Различие каналов — одна ветка внутри, а не два
     * публичных метода: снаружи «отправить» означает одно и то же.
     */
    fun send(key: String, raw: String, group: Boolean = false, mentions: List<Mention> = emptyList()) {
        // Хвостовые пробелы и переводы строк — случайность (лишний Enter): с ними время в
        // пузыре уезжает на пустую строку. Начало не трогаем — от него считаются упоминания.
        val text = raw.trimEnd()
        if (text.isNotBlank()) chimeSent()
        if (group) {
            sendGroup(key, text, mentions)
            return
        }
        if (text.isBlank()) return
        // Цель ответа снимаем СРАЗУ: пользователь уже нажал «отправить», и оставленная
        // цитата в поле ввода означала бы, что следующее сообщение тоже уйдёт ответом.
        val reply = replyTarget
        replyTarget = null
        ops.execute {
            // Своё ложится в хвост: лента-окно посреди переписки сначала к нему возвращается.
            if (detached[chatKey(key, group = false)] == true) loadHistory(key)
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
                // Движок не смог даже поставить сообщение в очередь (сбой базы):
                // показываем честно, текст не теряется с экрана.
                log(key).add(
                    Message(
                        --localMsgId,
                        text,
                        mine = true,
                        time = now,
                        status = "failed",
                        replyTo = reply?.id ?: 0,
                        sender = me?.pubkey.orEmpty(),
                        replySender = replyAuthor(key, reply),
                    ),
                )
            }
            markSummariesDirty() // своё сообщение — тоже последнее в списке чатов
        }
    }

    // ── Действия с сообщением (ADR-006) ───────────────────────────────────────

    /**
     * Что выделено в открытом чате. Одно на все виды чатов: открыт всегда ровно
     * один, и держать отдельный набор для группы значило бы синхронизировать два
     * состояния там, где хватает одного.
     */
    var selection by mutableStateOf<Set<MsgRef>>(emptySet())
        internal set

    /** На какое сообщение отвечаем (null — обычная отправка). */
    var replyTarget by mutableStateOf<Message?>(null)
        private set

    /** Открытое меню у пузыря; null — закрыто. */
    var bubbleActions by mutableStateOf<BubbleActions?>(null)

    fun toggleSelect(m: Message) {
        // Сообщения без id (старый клиент) выбрать нельзя: с ними нечего делать —
        // ни отозвать, ни ответить, а «удалить у себя» без ссылки не адресуется.
        if (m.id == 0L) return
        val ref = MsgRef(m.id, m.sender)
        selection = if (ref in selection) selection - ref else selection + ref
    }

    /** Забыть всё, что относится к прежде открытому чату. */
    fun leaveChat() {
        selection = emptySet()
        replyTarget = null
        editTarget = null
        bubbleActions = null
        jumpTo = null // недостигнутая цель прошлого чата не должна сработать в новом
        chatSearch = null
        feedMarks = FeedMarks()
        flash = null
        feedAtBottom = true
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
    /** Текст выделенного (для копирования) — из ленты любого чата. */
    fun selectedTextOf(id: String, group: Boolean = false): String {
        val sel = selection
        return chat(id, group)
            .filter { MsgRef(it.id, it.sender) in sel }
            .joinToString("\n") { if (it.text.isNotEmpty()) plainText(it) else it.name }
    }

    fun selectedText(key: String): String = selectedTextOf(key)

    /** Удалить выделенное только у себя — со снекбаром «Отменить»: до конца отсчёта
     *  сообщения только спрятаны, в движок удаление уходит потом. */
    fun deleteSelected(key: String, group: Boolean = false) {
        val refs = selection.toList()
        selection = emptySet()
        deleteRefs(key, group, refs)
    }

    /** Удалить у себя названное (меню у пузыря — без режима выбора); со снекбаром «Отменить». */
    fun deleteRefs(key: String, group: Boolean, refs: List<MsgRef>) {
        if (refs.isEmpty()) return
        val plural = if (!group && isSaved(key)) R.plurals.undo_notes_deleted else R.plurals.undo_msgs_deleted
        deleteWithUndo(chatKey(key, group), refs, plural) {
            if (group) {
                Native.groupDeleteMessages(sessionPtr, key, wireGroupRefs(refs))
                // Ленту группы перечитает событие от движка.
            } else {
                Native.deleteMessages(sessionPtr, key, wireDmRefs(refs))
                reloadHistory(key)
            }
        }
    }

    /** Отозвать выделенные СВОИ сообщения у собеседника. Чужие не трогаем. */
    fun retractSelected(key: String, group: Boolean = false) {
        val refs = selection.toList()
        selection = emptySet()
        retractRefs(key, group, refs)
    }

    /** Отозвать у собеседников названные СВОИ сообщения. Чужие не трогаем. */
    fun retractRefs(key: String, group: Boolean, refs: List<MsgRef>) {
        val ids = refs.filter { isMine(it) }.map { it.id }
        if (ids.isEmpty()) return
        val json = ids.joinToString(prefix = "[", postfix = "]")
        ops.execute {
            if (group) {
                Native.groupRetractMessages(sessionPtr, key, json)
            } else {
                Native.retractMessages(sessionPtr, key, json)
                reloadHistory(key)
            }
            // Своё отозванное могло быть последним в чате — строка списка тоже меняется.
            markSummariesDirty()
        }
    }

    /** Перечитать историю чата после удаления/отзыва.
     *
     *  Именно [loadHistory], а не «очистить и долить»: он ЗАМЕНЯЕТ список целиком и
     *  возвращает в ленту неотправленное из офлайн-очереди (его в БД нет). Ручная
     *  чистка на месте оставляла в ленте старые копии — сообщения задваивались. */
    private fun reloadHistory(key: String) = loadHistory(key)

    /** Отправить вложение с метаданными для карточки (длительность/кадр/превью).
     *  Их извлекает клиент — движок медиа не декодирует. */
    /**
     * Отправить вложение. Различие каналов — одна ветка внутри: снаружи это одно
     * действие, и полоска «отвечаем на …» снимается одинаково для обоих.
     */
    fun sendMediaWithMeta(
        key: String,
        content: ByteArray,
        mime: String,
        name: String,
        meta: MediaMeta,
        caption: String = "",
        group: Boolean = false,
    ) = ops.execute {
        chimeSent()
        // Цель ответа снимаем в тот же момент, что и у текста: иначе полоска
        // «отвечаем на …» висела бы после отправки вложения, а ссылка терялась.
        val reply = replyTarget
        replyTarget = null
        if (group) {
            Native.groupSendMedia(
                sessionPtr, key, content, mime, name,
                meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
                reply?.id ?: 0, reply?.sender.orEmpty(),
            )
        } else {
            val id = Native.sendMediaWithMeta(
                sessionPtr, key, content, mime, name,
                meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
                reply?.id ?: 0, replyAuthor(key, reply),
            )
            if (id >= 0 && currentChat == key) loadHistory(key)
        }
        markSummariesDirty()
    }

    /** Отправить вложение ФАЙЛОМ (потоково). Файл должен пережить отдачу — движок
     *  читает из него чанки по запросу получателя. Своя зашифрованная копия
     *  считается долго, поэтому на [heavy]: текст, отправленный следом, может
     *  уйти раньше файла — как и подобает тому, что уже готово. */
    fun sendMediaFile(
        key: String,
        path: String,
        mime: String,
        name: String,
        meta: MediaMeta,
        caption: String = "",
        group: Boolean = false,
    ) {
        chimeSent()
        // Цель ответа снимаем сразу, в момент нажатия — как у текста.
        val reply = replyTarget
        replyTarget = null
        heavy.execute { sendMediaFileNow(key, path, mime, name, meta, caption, group, reply) }
    }

    private fun sendMediaFileNow(
        key: String,
        path: String,
        mime: String,
        name: String,
        meta: MediaMeta,
        caption: String,
        group: Boolean,
        reply: Message?,
    ) {
        if (group) {
            Native.groupSendMediaFile(
                sessionPtr, key, path, mime, name,
                meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
                reply?.id ?: 0, reply?.sender.orEmpty(),
            )
        } else {
            val id = Native.sendMediaFile(
                sessionPtr, key, path, mime, name,
                meta.durationMs, meta.width, meta.height, meta.thumb ?: ByteArray(0), caption,
                reply?.id ?: 0, replyAuthor(key, reply),
            )
            if (id >= 0 && currentChat == key) loadHistory(key)
        }
        markSummariesDirty()
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
        rotation = RotationState(secondsLeft = ROTATE_GAP_SECS, total = recipients.size)
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
     *  Готовность придёт событием `blob_received` — история перечитается. [name], [size] и
     *  [chat] — для уведомления о передаче (без них докачка идёт молча). */
    fun downloadMedia(peer: String, blobId: String, name: String = "", size: Long = 0, chat: ChatTarget? = null) {
        downloads[blobId] = DOWNLOAD_INIT
        if (chat != null) {
            appContext?.let { TransferNotifier.cancelResult(it, blobId) }
            transfers[blobId] = TransferNotifier.Item(blobId, name.ifBlank { blobId.take(12) }, size, chat, peer, peerName(peer))
            refreshTransfers()
        }
        ops.execute { Native.downloadMedia(sessionPtr, peer, blobId) }
    }

    /** Докачки, заказанные в этой сессии, — для уведомления о передаче. Только главный поток. */
    private val transfers = HashMap<String, TransferNotifier.Item>()

    /** Не удавшиеся — для «Повторить». */
    private val failedTransfers = HashMap<String, TransferNotifier.Item>()

    /** Уведомление о передаче — по текущему прогрессу и тому, в сети ли отправитель. */
    private fun refreshTransfers() {
        val ctx = appContext ?: return
        if (transfers.isEmpty() && !transfersShown) return
        transfersShown = transfers.isNotEmpty()
        val items = transfers.values.map { it.copy(frac = downloads[it.blobId] ?: DOWNLOAD_INIT, waiting = online[it.peer] == false) }
        TransferNotifier.show(ctx, items)
    }
    private var transfersShown = false

    /** «Отмена» в уведомлении: снять заказ, карточка снова предложит «Скачать». */
    fun cancelDownload(blobId: String) = onMain {
        downloads.remove(blobId)
        transfers.remove(blobId)
        refreshTransfers()
        ops.execute { Native.cancelDownload(sessionPtr, blobId) }
    }

    fun cancelAllDownloads() = onMain { transfers.keys.toList().forEach { cancelDownload(it) } }

    /** «Повторить» после ошибки — тот же заказ у того же отправителя. */
    fun retryDownload(blobId: String) = onMain {
        val item = failedTransfers.remove(blobId) ?: return@onMain
        appContext?.let { ctx -> TransferNotifier.cancelResult(ctx, blobId) }
        downloadMedia(item.peer, blobId, item.name, item.size, item.chat)
    }

    /** Код для своего QR (ADR-007): личность + подписанный текущий адрес, если он
     *  менялся, + самоназвание. Читается с движка: формат — контракт движка.
     *  Состояние, а не вызов из UI: профиль держал код в `remember(ключ, имя)` и
     *  после смены адреса показывал СТАРЫЙ — по нему до нас уже не достучаться.
     *  Обновляется на старте, при смене имени и адреса. */
    var contactCode by mutableStateOf("")
        private set

    /** Что за код обмена — до решения (экран «Добавить контакт»). */
    fun inspectCode(code: String, done: (CodeInfo) -> Unit) = ops.execute {
        val info = runCatching {
            val o = JSONObject(Native.inspectCode(sessionPtr, code))
            CodeInfo(o.getString("status"), o.getString("key"), o.getString("name"))
        }.getOrDefault(CodeInfo("invalid", "", ""))
        onMain { done(info) }
    }

    /** Добавить по коду и послать запрос с приветствием (7i); итог ("sent" | статус кода) — в [done]. */
    fun requestContact(code: String, name: String, greeting: String, done: (String) -> Unit) = ops.execute {
        val r = Native.requestContact(sessionPtr, code, name.trim(), greeting.trim())
        if (r == "sent") refreshContactsNow()
        onMain { done(r) }
    }

    /** Отметить сверенным «по набору» ([on]) или снять отметку. */
    fun setVerified(key: String, on: Boolean) = ops.execute {
        Native.setVerified(sessionPtr, key, on)
        refreshContactsNow()
    }

    /** Сверка по отсканированному [code]: итог ("match" | "mismatch" | "invalid") — в [done]. */
    fun verifyByCode(key: String, code: String, done: (String) -> Unit) = ops.execute {
        val r = Native.verifyByCode(sessionPtr, key, code)
        refreshContactsNow()
        onMain { done(r) }
    }

    fun rename(key: String, name: String) = ops.execute {
        Native.renameContact(sessionPtr, key, name)
        refreshContactsNow()
    }

    /** Удалить контакт вместе с перепиской (движок стирает и историю). */
    fun remove(key: String) {
        // Разговор с удаляемым — завершаем: звонок с тем, кого уже нет в контактах,
        // повис бы без кнопки и без имени.
        if (dmCallPeer == key) leaveRoom()
        ops.execute { removeNow(key) }
    }

    private fun removeNow(key: String) {
        Native.removeContact(sessionPtr, key)
        if (currentChat == key) currentChat = null
        forgetChatLocally(key)
        refreshContactsNow()
    }

    /** Очистить историю личного чата у себя (контакт остаётся). Вместе с ней уходит
     *  и неотправленное — иначе оно ушло бы позже и вернулось в пустой чат. */
    fun clearHistory(key: String) = ops.execute {
        Native.clearDmHistory(sessionPtr, key)
        forgetChatLocally(key)
    }

    /** Убрать из памяти клиента то, что движок уже стёр: лог, закрепы, счётчик. */
    private fun forgetChatLocally(key: String) {
        messages[chatKey(key, group = false)]?.clear()
        pinned.remove(key) // закрепы — по голому ключу собеседника
        unread.remove(key)
        refreshChatSummariesNow()
    }

    // ── Гейт контактов (запросы от незнакомцев) ────────────────────────────
    fun refreshRequests() = ops.execute { refreshRequestsNow() }

    private fun refreshRequestsNow() {
        val list = decodeList<ContactRequestDto>(Native.contactRequests(sessionPtr))
            .map {
                ContactRequest(
                    it.key,
                    it.fingerprint,
                    it.name,
                    it.greeting,
                    it.time,
                    lookalikeKey = it.lookalike?.key.orEmpty(),
                    lookalikeName = it.lookalike?.name.orEmpty(),
                )
            }
            .sortedByDescending { it.time }
        contactRequests.replaceWith(list)
        syncRequestNotifications()
    }

    /** Одобрить запрос: незнакомец становится контактом. */
    fun acceptRequest(key: String) = ops.execute {
        if (Native.acceptContactRequest(sessionPtr, key)) {
            refreshRequestsNow()
            refreshContactsNow()
            // Движок положил первое сообщение незнакомца в чат — показать его в списке.
            refreshChatSummariesNow()
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
        refreshBlockedNow()
    }

    /** Разблокировать: строка уходит из «Заблокированных», в контакты пир не возвращается. */
    fun unblock(key: String) = ops.execute {
        Native.unblockContact(sessionPtr, key)
        refreshBlockedNow()
    }

    private fun refreshBlockedNow() {
        val list = decodeList<BlockedPeerDto>(Native.blockedPeers(sessionPtr))
            .map { BlockedPeer(it.key, it.name, it.contact, it.time) }
        onMain { blocked.replaceWith(list) }
    }

    /** Применить одно событие из ядра. Вызывается из [pollThread] (фон). */
    private fun applyEvent(json: String) {
        if (json.isEmpty()) return
        // Разбор по ТИПУ, а не по строковым ключам: набор полей каждого события
        // зафиксирован в Dto.kt и сверяется компилятором с dto.rs.
        val ev = parseEvent(json)
        // Что меняет ПОСЛЕДНЕЕ сообщение чата или его статус — меняет и строку в
        // списке. Флаг, а не пересчёт на месте: он гасится один раз за пачку.
        when (ev) {
            is EngineEvent.MessageEv, is EngineEvent.Ack, is EngineEvent.Read,
            is EngineEvent.Retracted, is EngineEvent.GroupRetracted, is EngineEvent.MessageEdited,
            EngineEvent.GroupsChanged,
            is EngineEvent.Undelivered, is EngineEvent.Failed, is EngineEvent.HistoryPruned,
            -> markSummariesDirty()
            else -> {}
        }
        when (ev) {
            is EngineEvent.Presence -> {
                online[ev.peer] = ev.online
                inApp[ev.peer] = ev.online && ev.active
                if (contacts.none { it.key == ev.peer }) refreshContactsNow()
            }
            is EngineEvent.MessageEv -> {
                val m = ev.msg.toModel()
                if (!ev.group && contacts.none { it.key == ev.chat }) refreshContactsNow()
                // Миниатюра — отдельный мелкий блоб: подтянем, если уже есть. Байты
                // самого вложения лента возьмёт при показе.
                m.thumbId.takeIf { it.isNotEmpty() }?.let { cacheBlob(it) }
                val feed = if (ev.group) groupLog(ev.chat) else log(ev.chat)
                when {
                    // Лента открыта окном посреди переписки: новое легло бы под окно через
                    // дыру. Чужое принесёт подгрузка вниз, своё — возвращает к хвосту.
                    detached[chatKey(ev.chat, ev.group)] != true || feed.any { it.id == m.id && it.sender == m.sender } ->
                        upsert(feed, m)
                    m.mine -> toTail(ChatTarget(ev.chat, ev.group))
                }
                // Своё и системные строки (кто вошёл/вышел) непрочитанным не считаем:
                // счётчик обещает сообщения, за которыми стоит человек.
                // Звонок — только пропущенный: строка звонка приходит заново на каждое
                // изменение исхода, а «missed» у неё бывает один раз.
                val counts = if (m.isCall) m.text == "missed" else !m.system
                if (!ev.group && !m.mine) {
                    typingUntil.remove(ev.chat) // сообщение пришло — набор окончен
                    if (viewing == ChatTarget(ev.chat, group = false) && foreground && counts) {
                        ops.execute { Native.markRead(sessionPtr, ev.chat) }
                    }
                }
                // Лента этого чата на экране: прочитано сразу, а прокрученная от хвоста —
                // копит пришедшее на капсуле (5.7). Чат «текущий», но не на экране (ушли в
                // профиль или на главную) — пришедшее непрочитанное.
                val onScreen = viewing == ChatTarget(ev.chat, ev.group)
                if (onScreen && !m.mine && counts) noteIncoming(m, ev.group)
                notifyIncoming(ChatTarget(ev.chat, ev.group), m, onScreen, ev.sent)
                if (onScreen && foreground && !m.mine && !m.system && !m.isCall) {
                    val mentionsMe = ev.group && m.mentions.any { it.key == me?.pubkey }
                    val quiet = !notifyMode(ev.chat, ev.group).lets(mentionsMe)
                    appContext?.let { ChatSounds.play(it, ChatSounds.Kind.In, chatSounds, quiet, ev.sent) }
                }
                // Свёрнутое приложение с открытым чатом — тоже непрочитанное: его считает «связь».
                if (!m.mine && counts && !(onScreen && foreground)) {
                    if (ev.group) {
                        groupUnread[ev.chat] = (groupUnread[ev.chat] ?: 0) + 1
                        if (m.mentions.any { it.key == me?.pubkey }) groupMentioned[ev.chat] = true
                    } else {
                        unread[ev.chat] = (unread[ev.chat] ?: 0) + 1
                    }
                    refreshConn()
                }
            }
            // Собеседник отозвал своё сообщение: движок уже удалил его из истории,
            // здесь убираем из открытой ленты, чтобы не ждать перезахода в чат.
            is EngineEvent.Retracted -> {
                val list = messages[chatKey(ev.peer, group = false)]
                val at = list?.indexOfFirst { it.id == ev.id && !it.mine } ?: -1
                if (at >= 0) list!![at] = list[at].retracted()
                appContext?.let { MessageNotifier.replace(it, ChatTarget(ev.peer, group = false), ev.peer, ev.id, null) }
            }
            is EngineEvent.Ack -> updateStatus(ev.peer, ev.id, "delivered")
            is EngineEvent.Read -> ev.ids.forEach { updateStatus(ev.peer, it, "read") }
            is EngineEvent.Typing -> typingUntil[ev.peer] = System.currentTimeMillis() + TYPING_SHOWN_MS
            // Ретраи в памяти выдохлись, сообщение ждёт в очереди движка и уйдёт,
            // когда собеседник появится.
            is EngineEvent.Undelivered -> updateStatus(ev.peer, ev.id, "queued")
            is EngineEvent.Failed -> updateStatus(ev.peer, ev.id, "failed")
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
                    val caller = contacts.firstOrNull { it.key == ev.peer }
                    val callerName = caller?.displayName ?: appContext?.getString(R.string.common_contact).orEmpty()
                    val avatar = caller?.avatarId?.let { contactAvatars[it] }
                    appContext?.let {
                        IncomingCallReceiver.show(it, callerName, ev.video, avatar)
                        checkFullScreenCalls(it)
                    }
                }
            }
            is EngineEvent.RoomDecline -> {
                if (ev.peer == dmCallPeer) {
                    // Сбросил, пока дозванивались, — «занято»; из идущего разговора — конец.
                    val dialing = roomMembers.none { it.peer == ev.peer }
                    leaveRoom()
                    if (dialing) CallTones.busy()
                }
                // Звонящий сбросил до того как мы ответили — гасим входящий; это пропущенный.
                val ic = incomingCall
                if (ic?.peer == ev.peer) {
                    incomingCall = null
                    appContext?.let { IncomingCallReceiver.cancel(it) }
                    if (!isGroupRoom(ic.roomId)) notifyMissedCall(ic.peer, ic.video)
                }
            }
            is EngineEvent.RoomPresence -> if (ev.gid == currentRoom) {
                // Мгновенно отражаем участника, не дожидаясь опроса тикером.
                if (ev.left) {
                    roomMembers.removeAll { it.peer == ev.peer } // явный выход — убрать сразу
                } else {
                    if (ev.peer == dmCallPeer) CallTones.stop() // ответил — гудки кончились
                    val rm = RoomMember(ev.peer, ev.flags and 0b01 != 0, ev.flags and 0b10 != 0)
                    val idx = roomMembers.indexOfFirst { it.peer == ev.peer }
                    if (idx >= 0) roomMembers[idx] = rm else roomMembers.add(rm)
                    appContext?.let { CallService.refresh(it) }
                }
            }
            // Гейт: незнакомец постучался — обновить очередь запросов (карточка в UI).
            is EngineEvent.ContactRequestEv -> {
                refreshRequestsNow()
                // Новый (не повтор того же) — в шторку; повторы движок помечает new = false.
                if (ev.new && notifyRequests) {
                    val r = contactRequests.firstOrNull { it.key == ev.peer }
                    appContext?.let { ctx -> r?.let { RequestNotifier.request(ctx, it, AppLock.enabled || !notifyText) } }
                }
            }
            // Приглашение в группу пришло/принято/отклонено — обновить карточки.
            EngineEvent.GroupInvitesChanged -> {
                refreshGroupInvitesNow()
                notifyNewInvites()
            }
            // Закреп общий для всех: его мог поставить собеседник или админ группы.
            is EngineEvent.PinsChanged -> refreshPins(ev.chat, groups.any { it.gid == ev.chat })
            // Правка приходит и на свои сообщения (эхо движка), и на чужие: меняем
            // текст на месте, чтобы не перечитывать всю ленту.
            is EngineEvent.Reactions -> {
                messages[chatKey(ev.chat, ev.group)]?.let { list ->
                    val i = list.indexOfFirst { it.id == ev.id && it.sender == ev.sender }
                    if (i >= 0) list[i] = list[i].copy(reactions = ev.reactions)
                }
                // Новая чужая реакция на моё в открытом чате — на «❤».
                if (openChatTarget() == ChatTarget(ev.chat, ev.group)) loadUnseenReactions(ChatTarget(ev.chat, ev.group))
                ev.news?.let { notifyReaction(ChatTarget(ev.chat, ev.group), it) }
                markSummariesDirty() // строка реакции в списке чатов
            }
            is EngineEvent.MessageEdited -> {
                val stamp = System.currentTimeMillis() / 1000
                messages[chatKey(ev.chat, group = false)]?.let { list ->
                    val i = list.indexOfFirst { it.id == ev.id && it.sender == ev.sender }
                    if (i >= 0) list[i] = list[i].copy(text = ev.text, editedAt = stamp)
                }
                messages[chatKey(ev.chat, group = true)]?.let { list ->
                    val i = list.indexOfFirst { it.id == ev.id && it.sender == ev.sender }
                    // Упоминания заменяются вместе с текстом: их участки привязаны к нему.
                    if (i >= 0) list[i] = list[i].copy(text = ev.text, editedAt = stamp, mentions = ev.mentions)
                }
                appContext?.let { ctx ->
                    val text = Markdown.plain(Mentions.plain(ev.text, ev.mentions, ::mentionName))
                    listOf(false, true).forEach { g -> MessageNotifier.replace(ctx, ChatTarget(ev.chat, g), ev.sender, ev.id, text) }
                }
            }
            // M2: контакт прислал профиль (имя/аватар) → перечитать список.
            EngineEvent.ContactsChanged -> refreshContactsNow()
            // Адрес сменился: обновляем показ и завершаем карточку хода.
            is EngineEvent.TransportRotated -> {
                transportEpoch = ev.epoch
                offline = false
                rotation = rotation?.copy(secondsLeft = 0, newAddress = ev.address)
                me = me?.copy(address = ev.address)
                contactCode = Native.myContactCode(sessionPtr) // в коде — подписанный адрес
            }
            // Новый узел не поднялся: карточку хода закрываем и говорим, что вышло.
            // Адрес прежний; если и на него вернуться не удалось — мы офлайн, и
            // вернёт сеть обычная кнопка «онлайн».
            is EngineEvent.TransportRotateFailed -> {
                rotation = null
                offline = !ev.online
                val msg = if (ev.online) R.string.rotate_failed else R.string.rotate_failed_offline
                appContext?.let { ctx ->
                    android.os.Handler(android.os.Looper.getMainLooper()).post {
                        android.widget.Toast.makeText(ctx, ctx.getString(msg), android.widget.Toast.LENGTH_LONG).show()
                    }
                }
            }
            is EngineEvent.BlobProgress -> {
                if (ev.total > 0) downloads[ev.id] = ev.done / ev.total.toFloat()
                onMain { if (ev.id in transfers) refreshTransfers() }
            }
            // Докачка не началась — карточка снова предлагает «Скачать», а человек
            // узнаёт причину, а не смотрит на вечный кружок.
            is EngineEvent.HistoryPruned -> reloadAfterPrune()
            is EngineEvent.BlobFailed -> {
                downloads.remove(ev.id)
                val noSpace = ev.reason == "no_space"
                val msg = if (noSpace) R.string.download_no_space else R.string.download_too_large
                appContext?.let { ctx ->
                    android.os.Handler(android.os.Looper.getMainLooper()).post {
                        android.widget.Toast.makeText(ctx, ctx.getString(msg), android.widget.Toast.LENGTH_LONG).show()
                    }
                    // Ошибка передачи — в шторку всегда; «Повторить» — где повтор поможет.
                    onMain {
                        transfers.remove(ev.id)?.let { item ->
                            if (noSpace) failedTransfers[ev.id] = item
                            TransferNotifier.failed(ctx, item, retry = noSpace, reason = ctx.getString(msg))
                            refreshTransfers()
                        }
                    }
                }
            }
            // Докачался блоб: аватар контакта — перечитать контакты; вложение чата —
            // положить байты в реактивный кэш (пузырь перерисуется).
            is EngineEvent.BlobReceived -> {
                downloads.remove(ev.id)
                // «Скачано» — только ручное и только когда приложение свёрнуто.
                onMain {
                    transfers.remove(ev.id)?.let { item ->
                        appContext?.let { ctx -> if (!foreground) TransferNotifier.done(ctx, item) }
                        refreshTransfers()
                    }
                }
                savedBlobs[ev.id] = true
                // Пометка «скачано» — на месте во всех открытых лентах: блоб
                // контент-адресный, один файл мог прийти в несколько чатов.
                for (list in messages.values) {
                    for (i in list.indices) {
                        val m = list[i]
                        if (m.blobId == ev.id && !m.downloaded) list[i] = m.copy(downloaded = true)
                    }
                }
                if (contacts.any { it.avatarId == ev.id }) refreshContactsNow()
                if (groups.any { it.photo == ev.id }) refreshGroupsNow()
                // Байты в память — только тому, что рисуется из памяти (фото,
                // голосовое, аватар). Докачанное видео на гигабайт сюда не читаем.
                if (ev.size <= INLINE_BLOB_MAX) reads.execute { cacheBlob(ev.id) }
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
            is EngineEvent.GroupRetracted -> {
                loadGroupHistory(ev.gid)
                // Какую строку отозвали, событие не говорит — снять уведомление группы целиком:
                // отозванное в шторке оставаться не должно.
                appContext?.let { MessageNotifier.cancel(it, ChatTarget(ev.gid, group = true)) }
            }
            // Событие, которого этот клиент ещё не знает: молча пропускаем.
            EngineEvent.Unknown -> Unit
        }
    }

    /**
     * «Повторить» не доставленное. Выброшенное из очереди (срок, сброс) возвращается в
     * неё с тем же id; не попавшее даже в очередь (сбой базы, локальный id < 0) уходит
     * заново, а прежняя строка убирается.
     */
    fun retry(key: String, m: Message) {
        if (m.id < 0) {
            messages[chatKey(key, group = false)]?.removeAll { it.id == m.id && it.mine }
            send(key, m.text)
            return
        }
        updateStatus(key, m.id, "pending")
        ops.execute { if (!Native.retryMessage(sessionPtr, key, m.id)) loadHistory(key) }
    }

    private fun updateStatus(peer: String, id: Long, status: String) {
        val l = messages[chatKey(peer, group = false)] ?: return
        val idx = l.indexOfFirst { it.id == id && it.mine }
        // «Прочитано» — последнее слово: запоздалый Ack его не откатывает (как в движке).
        if (idx >= 0 && l[idx].status != "read") l[idx] = l[idx].copy(status = status)
    }

    /** Поле ввода личного чата растёт: «печатаю» (выключено, офлайн, частота — решает движок). */
    fun typing(key: String) = ops.execute { Native.typing(sessionPtr, key) }

    fun setReadReceipts(on: Boolean) = ops.execute {
        Native.setReadReceipts(sessionPtr, on)
        privacy = privacy.copy(readReceipts = on)
    }

    fun setInAppStatus(on: Boolean) = ops.execute {
        Native.setInAppStatus(sessionPtr, on)
        privacy = privacy.copy(inApp = on)
        // Взаимно: скрывший чужого «в сети» не видит. Включил — придёт с ближайшим keepalive.
        if (!on) inApp.clear()
    }

    fun setTypingIndicator(on: Boolean) = ops.execute {
        Native.setTypingIndicator(sessionPtr, on)
        privacy = privacy.copy(typing = on)
        if (!on) typingUntil.clear()
    }

    // ── DM-звонки (через комнату) ─────────────────────────────────────────────

    /** Позвонить пиру: создать DM-комнату, отправить приглашение, начать медиа. */
    fun callDm(peer: String, audio: Boolean = true, video: Boolean = false) {
        if (dmCallPeer == peer) return // уже говорим (или дозваниваемся) с ним
        val ctx = appContext ?: return
        // Уже в другом звонке — переключаемся: старый закрываем. Раньше кнопка
        // молча ничего не делала, пока человек сидел в какой-нибудь комнате.
        val leaving = endCurrentCall()
        val name = contacts.firstOrNull { it.key == peer }?.displayName ?: ""
        startCall(leaving) {
            val roomId = Native.roomInviteDm(sessionPtr, peer, audio, video)
            if (roomId.isEmpty()) return@startCall
            currentRoom = roomId
            dmCallPeer = peer
            roomMembers.clear()
            CallService.start(ctx, name)
            startRoomMedia(ctx, roomId, video)
            CallTones.ringback() // до входа собеседника — см. RoomPresence
        }
    }

    /** Принять входящий звонок с ПРЕСЕТОМ своего mic/video (выбор join'ера до
     *  входа, НЕ флаги звонящего). Дефолт — мик вкл, видео выкл. Зовут либо в
     *  личную комнату, либо в комнату группы — туда и входим. */
    fun acceptDmCall(audio: Boolean = true, video: Boolean = false) {
        val ic = incomingCall ?: return
        incomingCall = null
        val ctx = appContext ?: return
        IncomingCallReceiver.cancel(ctx)
        if (currentRoom == ic.roomId) return // уже в этой комнате
        // Принять, сидя в другом звонке, — значит переключиться. Без этого новый
        // звонок вставал поверх старого: два захвата микрофона, а старая комната
        // так и не получала «вышел».
        val leaving = endCurrentCall()
        muted = !audio // мик-пресет → стартовое состояние мьюта
        if (isGroupRoom(ic.roomId)) {
            enterRoom(ic.roomId, audio, video, leaving)
            openGroupRequest = ic.roomId
            return
        }
        val name = contacts.firstOrNull { it.key == ic.peer }?.displayName ?: ""
        startCall(leaving) {
            val roomId = Native.roomJoinDm(sessionPtr, ic.peer, audio, video)
            if (roomId.isEmpty()) return@startCall
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
        if (isGroupRoom(ic.roomId)) {
            ops.execute { Native.roomDeclineGroup(sessionPtr) }
        } else {
            ops.execute { Native.roomDeclineDm(sessionPtr, ic.peer) }
        }
    }

    /** Комната — групповая (её id = gid группы), а не личная. */
    fun isGroupRoom(roomId: String): Boolean = groups.any { it.gid == roomId }

    /** Кого из группы можно позвать в текущую комнату: ростер минус те, кто уже тут.
     *  В личном звонке звать некого — там нет ростера. */
    fun ringCandidates(): List<String> {
        val gid = currentRoom ?: return emptyList()
        if (dmCallPeer != null) return emptyList()
        val here = roomMembers.map { it.peer }.toSet()
        return (groupMemberKeys[gid] ?: emptyList()).filter { it !in here }
    }

    /** Позвать выбранных участников группы в текущую комнату. */
    fun ringIntoRoom(peers: List<String>) {
        val gid = currentRoom ?: return
        if (peers.isEmpty()) return
        val audio = !muted
        val video = videoActive
        ops.execute {
            val sent = Native.roomRing(sessionPtr, gid, peers.joinToString(","), audio, video)
            val ctx = appContext ?: return@execute
            val text = if (sent > 0) {
                ctx.resources.getQuantityString(R.plurals.call_ring_sent, sent, sent)
            } else {
                ctx.getString(R.string.call_ring_failed)
            }
            android.os.Handler(android.os.Looper.getMainLooper()).post {
                android.widget.Toast.makeText(ctx, text, android.widget.Toast.LENGTH_SHORT).show()
            }
        }
    }

    /** Создать аудио-сессию комнаты (Opus-битрейт/режим + fx + устройство из настроек).
     *  Используется при старте звонка и при «Применить» в настройках звука (рестарт). */
    private fun startRoomAudioSession(ctx: android.content.Context, gid: String) {
        // Потолок битрейта и режим Opus — из «Качества звука»; ниже потолка движок
        // держит битрейт по оценке канала.
        val params = decodeOrNull<CallParamsDto>(
            Native.roomAudioStart(sessionPtr, gid, audioMode.bitrate, audioMode.music),
        )
        val sr = params?.sampleRate?.takeIf { it > 0 } ?: 48_000
        val ch = params?.channels?.takeIf { it > 0 } ?: 1
        val audio = CallAudio(
            ctx,
            { data, n, ageUs -> Native.roomPushAudio(sessionPtr, data, n, ageUs) },
            { out, delayUs -> Native.roomPullAudio(sessionPtr, out, delayUs) },
            { m -> Native.roomSetMuted(sessionPtr, m) },
            fx = audioFx,
            inputDevice = selectedInputDevice(),
            onLevel = { lvl -> if (showAudioSettings) micLevel = lvl }, // только когда экран открыт
        )
        // Микрофон могут отобрать (разрешение снято в настройках, его держит другое
        // приложение) — тогда AudioRecord бросает. Исключение в потоке ops роняет
        // весь процесс; звонок без своего звука лучше, чем падение посреди входа.
        roomAudio = try {
            audio.start(sr, ch)
            audio.setMuted(muted) // mic-пресет применяем сразу
            audio
        } catch (e: Exception) {
            android.util.Log.w("call", "audio start failed: ${e.javaClass.simpleName}")
            runCatching { audio.stop() }
            null
        }
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
            ops.execute { Native.roomSetAudioBitrate(sessionPtr, audioMode.bitrate) }
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
            onRotation = { rot -> roomMemberRotation[peerHex] = rot },
            requestKeyframe = { Native.roomRequestKeyframe(sessionPtr, peerHex) },
            reportDelay = { us -> Native.roomSetVideoDelay(sessionPtr, peerHex, us) },
        ) { waitUs -> Native.roomPullVideoPacket(sessionPtr, peerHex, waitUs) }.also { it.start() }
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
        appContext?.let { CallService.refresh(it) }
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
        // Сюда приходят с главного потока (колбэк surface превью) — нативный вызов
        // уводим в ops (инвариант 4).
        ops.execute { Native.roomSetVideoBitrate(sessionPtr, videoQuality.bitrate) }
        // Стартуем с текущей стороны камеры (cameraFront): дефолт — фронталка, но
        // экран шаринга мог выбрать тыловую (applyVideoShare ставит cameraFront).
        val vs = VideoSender(
            ctx,
            videoQuality.width,
            videoQuality.height,
            videoQuality.fps,
            videoQuality.bitrate,
            startFront = cameraFront,
            targetBitrate = { Native.roomVideoTargetBitrate(sessionPtr) },
        ) { data, key, rotation, fps, ageUs -> Native.roomPushVideo(sessionPtr, data, key, rotation, fps, ageUs) }
        // Кодер может не принять разрешение/битрейт (configure бросает) — на главном
        // потоке это падение всего приложения. Не вышло — гасим видео, звонок живёт.
        videoSender = try {
            vs.start(preview)
            vs
        } catch (e: Exception) {
            android.util.Log.w("call", "video start failed: ${e.javaClass.simpleName}")
            runCatching { vs.stop() }
            videoActive = false
            null
        }
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
        if (currentRoom == gid) return
        // Из другого звонка — сначала выйти: иначе старый звонок оставался жить
        // (его аудио никто не останавливал, комната не получала «вышел»).
        enterRoom(gid, audio, video, endCurrentCall())
    }

    private fun enterRoom(gid: String, audio: Boolean, video: Boolean, after: Thread?) {
        currentRoom = gid
        dmCallPeer = null
        roomMembers.clear()
        val ctx = appContext ?: return
        val name = groups.firstOrNull { it.gid == gid }?.name ?: ""
        startCall(after) {
            Native.roomJoin(sessionPtr, gid, audio, video)
            CallService.start(ctx, name, room = true)
            startRoomMedia(ctx, gid, video)
        }
    }

    /** Выйти из комнаты: стоп медиа + маячок «вышел». */
    fun leaveRoom() {
        endCurrentCall()
    }

    /** Разобрать текущий звонок. Возвращает поток разбора (или null, если звонка
     *  не было) — старт следующего звонка его дожидается, см. [startCall]. */
    private fun endCurrentCall(): Thread? {
        val gid = currentRoom ?: return null
        // Запомнить ДО очистки. peerPresent: активный звонок (пир в комнате) vs
        // ещё дозваниваемся (пира в комнате нет) — от этого зависит, слать ли decline.
        val peer = dmCallPeer
        val peerPresent = peer != null && roomMembers.any { it.peer == peer }
        CallTones.stop()
        // Конец разговора — тон; отменённый дозвон и комната группы без собеседника — тихо.
        if (peerPresent || (peer == null && roomMembers.isNotEmpty())) CallTones.ended()
        currentRoom = null
        dmCallPeer = null
        videoActive = false
        showRingSheet = false
        roomMembers.clear()
        muted = false
        micLevel = 0f // вывод (audioOut) теперь персистится как настройка — не сбрасываем
        callGen.incrementAndGet() // отложенный старт этого звонка уже не начнётся
        val vrs = videoReceivers.values.toList()
        videoReceivers.clear()
        val ctx = appContext
        // Мимо ops — см. [callGen]. Сначала гасим отправку и захват, потом остальное.
        return Thread {
            synchronized(callLock) {
                Native.roomAudioStop(sessionPtr)
                val a = roomAudio
                roomAudio = null
                val vs = videoSender
                videoSender = null
                runCatching { a?.stop() }
                runCatching { vs?.stop() }
                // RoomDecline шлём ТОЛЬКО чтобы отменить ДОЗВОН (пир ещё не вошёл) → у
                // него гаснет экран входящего+уведомление. Выход из АКТИВНОЙ комнаты —
                // только presence LEFT (roomLeave): пир ОСТАЁТСЯ в комнате (комнатная
                // модель), мы не «кладём ему трубку».
                if (peer != null && !peerPresent) Native.roomDeclineDm(sessionPtr, peer)
                Native.roomLeave(sessionPtr, gid)
                if (ctx != null) CallService.stop(ctx)
            }
            vrs.forEach { runCatching { it.stop() } }
        }.apply { isDaemon = true }.also { it.start() }
    }

    // ── Группы ──────────────────────────────────────────────────────────────

    fun refreshGroups() = ops.execute { refreshGroupsNow() }

    private fun refreshGroupsNow() {
        val list = decodeList<GroupRowDto>(Native.groupList(sessionPtr))
            .map {
                val default = NotifyMode.parse(it.notifyDefault) ?: NotifyMode.All
                Group(it.gid, it.name, it.members, it.admin, it.archived, it.adminKey, it.photo, default)
            }
        groups.replaceWith(list)
        // Фото групп — в тот же кэш, что аватары контактов: ключ — контент-адрес блоба.
        list.forEach { g ->
            if (g.photo.isNotEmpty() && !contactAvatars.containsKey(g.photo)) {
                Native.blobRead(sessionPtr, g.photo).takeIf { it.isNotEmpty() }?.let { contactAvatars[g.photo] = it }
            }
        }
    }

    // ── Сводка чатов для главной ────────────────────────────────────────────

    /** Строки списка чатов по ключу `dm:<hex>` / `g:<gid>`. Заполняется движком:
     *  последнее сообщение, закреп и «без звука». Непрочитанные и присутствие —
     *  не отсюда ([unread], [online], [groupPresence]). */
    val chatSummaries = mutableStateMapOf<String, ChatSummary>()

    /** Ключ строки в [chatSummaries]: вид чата входит в ключ, потому что hex группы
     *  и hex контакта живут в одном пространстве имён и однажды совпали бы. */
    fun chatKey(id: String, group: Boolean): String = if (group) "g:$id" else "dm:$id"

    /** Взведён — сводку надо перечитать. Гасится в [startLoops] после пачки событий. */
    @Volatile private var summariesDirty = false

    /** Пометить список чатов устаревшим (последнее сообщение/статус изменились). */
    internal fun markSummariesDirty() {
        summariesDirty = true
    }

    fun refreshChatSummaries() = ops.execute { refreshChatSummariesNow() }

    private fun refreshChatSummariesNow() {
        val rows = decodeList<ChatSummaryDto>(Native.chatSummaries(sessionPtr))
        val fresh = rows.associate { s ->
            val last = s.last
            chatKey(s.chat, s.group) to ChatSummary(
                id = s.chat,
                group = s.group,
                pinnedAt = s.pinnedAt,
                notify = NotifyMode.parse(s.notify),
                notifyUntil = s.notifyUntil,
                lastText = last?.text.orEmpty(),
                lastMentions = last?.mentions.orEmpty(),
                lastTime = last?.time ?: 0,
                lastMine = last?.mine ?: false,
                lastSender = last?.sender.orEmpty(),
                lastKind = last?.kind ?: "text",
                lastStatus = last?.status.orEmpty(),
                lastSystem = last?.system ?: false,
                reaction = s.reaction?.let { ChatReaction(it.by, it.emoji, it.quote, it.kind) },
            )
        }
        // Не clear+addAll: снапшот-карта уведомляет по ключам, и полная очистка
        // заставила бы список мигнуть пустотой на каждом входящем сообщении.
        chatSummaries.keys.retainAll(fresh.keys)
        chatSummaries.putAll(fresh)
        appContext?.let { ShareShortcuts.sync(it, this) }
    }

    /** Закрепить чат в списке / снять. Пометка локальная — собеседник не узнает. */
    fun pinChat(id: String, group: Boolean, on: Boolean) = ops.execute {
        Native.setChatPinned(sessionPtr, id, group, on)
        refreshChatSummariesNow()
    }

    /** Умолчание чата: у группы — от админа, у личного — все сообщения. */
    fun notifyDefault(id: String, group: Boolean): NotifyMode =
        if (group) groups.firstOrNull { it.gid == id }?.notifyDefault ?: NotifyMode.All else NotifyMode.All

    /** Действующий режим уведомлений чата: свой (пока не истёк) или умолчание. */
    fun notifyMode(id: String, group: Boolean): NotifyMode {
        val s = chatSummaries[chatKey(id, group)]
        return NotifyMode.effective(s?.notify, s?.notifyUntil ?: 0, notifyDefault(id, group), System.currentTimeMillis() / 1000)
    }

    /** Свой выбор, если ещё действует: `(режим, до)`; `null` — по умолчанию. */
    fun ownNotify(id: String, group: Boolean): Pair<NotifyMode, Long>? {
        val s = chatSummaries[chatKey(id, group)] ?: return null
        val own = s.notify ?: return null
        val live = s.notifyUntil == 0L || System.currentTimeMillis() / 1000 < s.notifyUntil
        return if (live) own to s.notifyUntil else null
    }

    /** Чат приглушён (вид в списке): режим не «все». */
    fun chatQuiet(id: String, group: Boolean): Boolean = notifyMode(id, group) != NotifyMode.All

    /**
     * Свой режим уведомлений чата (локально): `mode = null` — вернуть к умолчанию; `forSec` —
     * на сколько секунд (0 — пока не вернут).
     */
    fun setChatNotify(id: String, group: Boolean, mode: NotifyMode?, forSec: Long = 0) = ops.execute {
        val until = if (mode != null && forSec > 0) System.currentTimeMillis() / 1000 + forSec else 0
        Native.setChatNotify(sessionPtr, id, group, mode?.wire.orEmpty(), until)
        refreshChatSummariesNow()
    }

    /** «Заглушить на час» в шторке (notifications.md, п. 5). */
    fun muteForHour(chat: ChatTarget) =
        setChatNotify(chat.id, chat.group, NotifyMode.hourMute(chat.group), NotifyMode.DURATIONS.first())

    /** Умолчание уведомлений группы (только админ): уходит участникам. */
    fun setGroupNotifyDefault(gid: String, mode: NotifyMode) = ops.execute {
        Native.groupSetNotifyDefault(sessionPtr, gid, mode.wire)
    }

    // ── Приглашения в группы (карточки на одобрение) ───────────────────────
    fun refreshGroupInvites() = ops.execute { refreshGroupInvitesNow() }

    private fun refreshGroupInvitesNow() {
        val list = decodeList<GroupInviteDto>(Native.groupInvites(sessionPtr))
            .map { GroupInvite(it.gid, it.name, it.inviter, it.waiting) }
        groupInvites.replaceWith(list)
        syncRequestNotifications()
    }

    /** Решённое в приложении (или у собеседника) — из шторки долой. */
    private fun syncRequestNotifications() {
        appContext?.let { RequestNotifier.sync(it, contactRequests.map { r -> r.key }.toSet(), groupInvites.map { i -> i.gid }.toSet()) }
    }

    /**
     * Приглашения, которых шторка ещё не показывала. Флага «новое» у события нет, а сохранённые
     * движок объявляет заново после каждого запуска — поэтому показанные помним в настройках. Самый
     * первый раз запоминаем текущие молча: старые не должны всплыть после обновления.
     */
    private fun notifyNewInvites() {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
        val seen = prefs.getStringSet(KEY_SEEN_INVITES, null)
        val now = groupInvites.filterNot { it.waiting }
        if (seen != null && notifyRequests) {
            now.filter { it.gid !in seen }.forEach { RequestNotifier.invite(ctx, it, peerName(it.inviter)) }
        }
        // Помним только живые: принятое и отклонённое из набора уходит, набор не растёт.
        prefs.edit().putStringSet(KEY_SEEN_INVITES, groupInvites.map { it.gid }.toSet()).apply()
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
    /** Имя собеседника для показа — единственный способ получить его по ключу:
     *  локальное имя контакта обычно пустое ([Contact.localName]). */
    fun peerName(hex: String): String =
        contacts.firstOrNull { it.key == hex }?.displayName ?: (hex.take(8) + "…")

    /** Фото собеседника (или своё — по своему ключу); null — рисуем букву. */
    fun peerAvatar(hex: String): ByteArray? = if (hex == me?.pubkey) {
        selfAvatar
    } else {
        contacts.firstOrNull { it.key == hex }?.avatarId?.let { contactAvatars[it] }
    }

    fun loadGroupMembers(gid: String) = ops.execute {
        groupMemberKeys[gid] = decodeList<String>(Native.groupMembers(sessionPtr, gid))
    }

    /** Отозвать участника (ADR-004, только админ). Ростер обновит `groups_changed`. */
    fun removeGroupMember(gid: String, peerHex: String) = ops.execute {
        Native.groupRemoveMember(sessionPtr, gid, peerHex)
        loadGroupMembers(gid)
    }

    /** Выйти из группы (ADR-004). Локально забываем сразу; движок просит админа отозвать. */
    /** Выйти из группы; [heir] — кому админ передаёт права перед уходом (null — без передачи). */
    fun leaveGroup(gid: String, heir: String? = null) {
        if (currentGroup == gid) currentGroup = null
        // Сидим в звонке этой группы — выходим и из него: иначе маячок продолжал бы
        // объявлять нас в комнате группы, из которой мы ушли.
        if (currentRoom == gid) leaveRoom()
        ops.execute {
            if (heir != null) {
                Native.groupTransferAdmin(sessionPtr, gid, heir, true)
            } else {
                Native.groupLeave(sessionPtr, gid)
            }
        }
    }

    /** Передать права админа участнику [peerHex] (шаг 7j). */
    fun transferAdmin(gid: String, peerHex: String) = ops.execute {
        Native.groupTransferAdmin(sessionPtr, gid, peerHex, false)
    }

    /** Фото группы (только админ); пустые байты — убрать. Список обновит `groups_changed`. */
    fun setGroupPhoto(gid: String, content: ByteArray, mime: String) = ops.execute {
        Native.groupSetPhoto(sessionPtr, gid, content, mime)
    }

    /** Байты фото группы; null — фото нет или блоб ещё не докачан (тогда буква). */
    fun groupPhoto(gid: String): ByteArray? = groups.firstOrNull { it.gid == gid }?.photo?.let { contactAvatars[it] }

    /** Переименовать группу (только админ); список обновит `groups_changed`. */
    fun renameGroup(gid: String, name: String) = ops.execute { Native.groupRename(sessionPtr, gid, name.trim()) }

    /** Создать группу и позвать [invite] (hex-ключи) — одной командой движка. Появится в
     *  списке по событию `groups_changed`. */
    fun createGroup(name: String, invite: List<String> = emptyList()) = ops.execute {
        Native.groupCreate(
            sessionPtr,
            name.ifBlank { appContext?.getString(R.string.group_default_name) ?: "Group" },
            org.json.JSONArray(invite).toString(),
        )
    }

    /** Позванные в группу и ещё не вступившие (метка «приглашён»). [done] — на главном потоке. */
    fun groupInvited(gid: String, done: (Set<String>) -> Unit) = reads.execute {
        val keys = decodeOrNull<List<String>>(Native.groupInvited(sessionPtr, gid)).orEmpty().toSet()
        android.os.Handler(android.os.Looper.getMainLooper()).post { done(keys) }
    }

    fun openGroup(gid: String) {
        leaveChat()
        currentGroup = gid
        // Сколько пришло, пока чат был закрыт: среди этого ищутся упоминания для «@».
        val unreadN = groupUnread[gid] ?: 0
        groupUnread[gid] = 0
        groupMentioned.remove(gid)
        loadGroupMembers(gid)
        ops.execute {
            loadGroupHistory(gid)
            collectUnreadMentions(gid, unreadN)
        }
        loadUnseenReactions(ChatTarget(gid, group = true))
    }

    private fun loadGroupHistory(gid: String) {
        // Флаг `downloaded` из истории уточняем фактом наличия блоба: своё вложение
        // и всё скачанное до появления флага иначе показывалось бы карточкой
        // «скачать». Проверка не читает байты — только наличие файла.
        val list = decodeList<MessageDto>(Native.groupHistory(sessionPtr, gid)).map { it.toModel() }
            .map {
                if (!it.downloaded && it.blobId.isNotEmpty() && Native.blobExists(sessionPtr, it.blobId)) {
                    it.copy(downloaded = true)
                } else {
                    it
                }
            }
        messages[chatKey(gid, group = true)] = list.toMutableStateList()
        atChatStart -= "g:$gid"
        detached.remove(chatKey(gid, group = true))
        // Байты вложений и миниатюры подтянет `blob()` при показе — заранее
        // читать всю страницу с диска незачем.
    }

    internal fun groupLog(gid: String): SnapshotStateList<Message> =
        messages.getOrPut(chatKey(gid, group = true)) { mutableListOf<Message>().toMutableStateList() }

    /**
     * Строка из события: уже есть в ленте (та же пара «автор + id» — например, лента
     * успела перечитаться) — заменить на месте, иначе дописать в конец. Дубль
     * недопустим: по этой паре лента держит ключи элементов.
     */
    private fun upsert(list: SnapshotStateList<Message>, m: Message) {
        val i = list.indexOfFirst { it.id == m.id && it.sender == m.sender }
        if (i >= 0) list[i] = m else list.add(m)
    }

    fun inviteToGroup(gid: String, peerHex: String) = ops.execute { Native.groupInvite(sessionPtr, gid, peerHex) }

    private fun sendGroup(gid: String, text: String, mentions: List<Mention>) {
        if (text.isBlank()) return
        val reply = replyTarget
        replyTarget = null
        ops.execute {
            Native.groupSendTextReply(
                sessionPtr,
                gid,
                text,
                reply?.id ?: 0,
                reply?.sender.orEmpty(),
                mentionsJson(mentions),
            )
            // Строку в ленту добавит событие `message` от движка: id сообщению
            // присваивает поток-актор, и сразу после возврата из JNI его ещё нет.
        }
    }

    // ── Действия с групповым сообщением (ADR-006) ────────────────────────────

    /** Сообщение, к которому просят прокрутить ленту (находка поиска, «показать в
     *  чате» из просмотрщика, переход к дате). Пара «автор + id»: один id бывает у
     *  разных авторов. Лента сама гасит цель после прокрутки — иначе повторный вход
     *  в чат снова уезжал бы к старому вложению. */
    var jumpTo by mutableStateOf<MsgRef?>(null)

    /** Капсула «❤ · @ · вниз» и стек возвратов открытого чата (5.7, [FeedMarks]). */
    var feedMarks by mutableStateOf(FeedMarks())

    /** Подсветка ряда после перехода по капсуле или цитате; гасит лента. */
    var flash by mutableStateOf<Flash?>(null)

    /** Лента открытого чата у хвоста — пришедшее видно сразу, на «вниз» не копится. Пишет лента. */
    @Volatile var feedAtBottom = true

    /**
     * Чья лента сейчас НА ЭКРАНЕ ([startViewing]/[stopViewing]). Не то же, что [currentChat]:
     * тот остаётся выбранным и на профиле, и после «назад» на главную, — и пришедшее в него
     * не считалось непрочитанным, а в личном чате уходило «прочитано», которого не было.
     */
    // Состояние снимка, а не просто поле: по нему же список в две панели подсвечивает открытый
    // чат. Пишет главный поток (лента), читают и фоновые — чтение снимка с них безопасно.
    var viewing by mutableStateOf<ChatTarget?>(null)

    /** Поиск в открытом чате (2.9); null — не открыт. */
    var chatSearch by mutableStateOf<ChatSearch?>(null)

    /** Открытый экран «Медиа и файлы» (3.2); null — не открыт. */
    var mediaBrowse by mutableStateOf<MediaBrowse?>(null)

    /** Столбцов в сетке «Медиа» — 2, 3 или 5 (щипок; запоминается). */
    var mediaColumns by mutableStateOf(3)
        internal set

    /**
     * Ленты, открытые окном посреди переписки (переход к находке или к дате), а не с
     * хвоста: ниже окна ещё есть история. Новые сообщения в такую ленту не дописываются
     * — между ними и окном была бы дыра; их приносит подгрузка вниз ([loadNewer]),
     * а своё отправленное возвращает ленту к хвосту. Ключ — [chatKey].
     */
    internal val detached = mutableStateMapOf<String, Boolean>()

    /** Ленты с идущей подгрузкой вниз — прокрутка не запускает её пачкой. */
    private val loadingNewer = mutableSetOf<String>()

    /** Где пометка «дошли до начала переписки» у этого чата ([atChatStart]). */
    private fun startMark(chat: ChatTarget) = if (chat.group) "g:${chat.id}" else chat.id

    /** Страница истории из движка — как модель: у группы `downloaded` уточняется по
     *  наличию блоба (своё и скачанное до появления флага). */
    private fun pageOf(json: String, group: Boolean): List<Message> =
        decodeList<MessageDto>(json).map { it.toModel() }.map {
            if (group && !it.downloaded && it.blobId.isNotEmpty() && Native.blobExists(sessionPtr, it.blobId)) {
                it.copy(downloaded = true)
            } else {
                it
            }
        }

    /**
     * Показать в ленте место переписки: сообщение [target] (находка) или, без него, первое
     * сообщение не раньше [time] (`row = 0` — переход к дате). Уже в ленте — только
     * прокрутка; иначе лента открывается окном вокруг места.
     */
    fun showAt(chat: ChatTarget, time: Long, row: Long, target: MsgRef? = null) = ops.execute {
        val key = chatKey(chat.id, chat.group)
        val list = messages[key]
        if (target != null && list?.any { it.id == target.id && it.sender == target.sender } == true) {
            jumpTo = target
            return@execute
        }
        val page = pageOf(Native.historyAround(sessionPtr, chat.id, chat.group, time, row), chat.group)
        messages[key] = page.toMutableStateList()
        atChatStart -= startMark(chat)
        // Окно могло дотянуться до хвоста — тогда лента обычная, и новые дописываются.
        val bottom = page.lastOrNull()
        val more = bottom != null &&
            decodeList<MessageDto>(Native.historyAfter(sessionPtr, chat.id, chat.group, bottom.time, bottom.row)).isNotEmpty()
        if (more) detached[key] = true else detached.remove(key)
        jumpTo = target ?: page.firstOrNull { it.time >= time }?.let { MsgRef(it.id, it.sender) }
            ?: bottom?.let { MsgRef(it.id, it.sender) }
    }

    /** Подгрузить страницу НИЖЕ окна (лента открыта посреди переписки). Пустая — дошли
     *  до хвоста: лента снова обычная. */
    fun loadNewer(chat: ChatTarget) {
        val key = chatKey(chat.id, chat.group)
        val list = messages[key]
        val bottom = list?.lastOrNull { it.row != 0L }
        if (detached[key] != true || bottom == null || !loadingNewer.add(key)) return
        ops.execute {
            try {
                val page = pageOf(Native.historyAfter(sessionPtr, chat.id, chat.group, bottom.time, bottom.row), chat.group)
                if (page.isEmpty()) detached.remove(key) else list.addAll(withoutShown(list, page))
            } finally {
                loadingNewer -= key
            }
        }
    }

    /**
     * Автоудаление убрало старое: открытые ленты перечитываются целиком, закрытые
     * забываются (их перечитает вход в чат), кэш байтов сбрасывается — удалённый файл
     * иначе показывался бы из памяти.
     */
    internal fun reloadAfterPrune() = ops.execute {
        val dm = currentChat
        val gid = currentGroup
        val open = setOfNotNull(dm?.let { chatKey(it, group = false) }, gid?.let { chatKey(it, group = true) })
        onMain { messages.keys.filter { it !in open }.forEach { messages.remove(it) } }
        dm?.let { loadHistory(it) }
        gid?.let { loadGroupHistory(it) }
        mediaBlobs.clear()
        chat.peerage.ui.Bitmaps.clear()
        refreshStorage()
    }

    /** Вернуть ленту к хвосту переписки и встать на последнее сообщение. */
    fun toTail(chat: ChatTarget) = ops.execute {
        if (chat.group) loadGroupHistory(chat.id) else loadHistory(chat.id)
        jumpTo = messages[chatKey(chat.id, chat.group)]?.lastOrNull()?.let { MsgRef(it.id, it.sender) }
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
