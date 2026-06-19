package dev.p2p.messenger

/**
 * Мост к Rust-ядру (cdylib libp2p_android.so). Имена методов совпадают с
 * JNI-символами `Java_dev_p2p_messenger_Native_*` в androidjni/src/lib.rs.
 *
 * libygg.so (Yggdrasil) грузим первой — libp2p_android.so на неё ссылается.
 */
object Native {
    init {
        System.loadLibrary("c++_shared") // нужна libp2p_android.so (C++ openh264)
        System.loadLibrary("ygg")
        System.loadLibrary("p2p_android")
    }

    /** Инициализация. Возвращает JSON {pubkey, address, fingerprint, session_ptr} или {error}. */
    external fun start(dataDir: String): String

    /** Освобождение ресурсов сессии. */
    external fun destroySession(sessionPtr: Long)

    /** Следующее событие как JSON-строка, либо "" если очередь пуста. */
    external fun pollEvent(sessionPtr: Long): String

    /** Отправить текст. Возвращает id сообщения (>=0) или -1 при ошибке. */
    external fun sendText(sessionPtr: Long, keyHex: String, text: String): Long

    /** Держать фоновое соединение с пиром. */
    external fun connect(sessionPtr: Long, keyHex: String)

    /** Добавить контакт (TOFU). Возвращает "added" | "known" | "error". */
    external fun addContact(sessionPtr: Long, keyHex: String, name: String): String

    /** Контакты как JSON-массив. */
    external fun listContacts(sessionPtr: Long): String

    /** История переписки с пиром как JSON-массив. */
    external fun history(sessionPtr: Long, keyHex: String): String

    external fun setVerified(sessionPtr: Long, keyHex: String, verified: Boolean)
    external fun renameContact(sessionPtr: Long, keyHex: String, name: String)
    external fun removeContact(sessionPtr: Long, keyHex: String)
    external fun isOnline(sessionPtr: Long, keyHex: String): Boolean
    external fun netPeers(sessionPtr: Long): Int
    external fun netTree(sessionPtr: Long): Int
    external fun stop(sessionPtr: Long)

    // ── Звонки (1:1) ──────────────────────────────────────────────────────
    /** Начать исходящий звонок. Возвращает JSON {call_id, sample_rate, channels, frame_len}. */
    external fun callStart(sessionPtr: Long, keyHex: String, video: Boolean, vw: Int, vh: Int): String
    /** Принять входящий звонок. Возвращает тот же JSON с аудио-параметрами. */
    external fun callAccept(sessionPtr: Long, keyHex: String, callId: Long, video: Boolean, vw: Int, vh: Int): String
    /** Завершить звонок. */
    external fun callEnd(sessionPtr: Long, keyHex: String, callId: Long)
    /** Захваченный PCM (первые n сэмплов) → отправка. */
    external fun callPushAudio(sessionPtr: Long, data: ShortArray, n: Int)
    /** Заполнить буфер PCM для воспроизведения. Возвращает число реальных сэмплов. */
    external fun callPullAudio(sessionPtr: Long, out: ShortArray): Int
    /** H.264 access-unit от MediaCodec-энкодера → фрагментация и отправка. */
    external fun callSendVideo(sessionPtr: Long, nal: ByteArray, keyframe: Boolean)
    /** Собранный входящий H.264 access-unit для MediaCodec-декодера (или пусто). */
    external fun callPullVideoPacket(sessionPtr: Long): ByteArray
    external fun callSetMuted(sessionPtr: Long, muted: Boolean)

    // ── Группы (MLS) ──────────────────────────────────────────────────────
    /** Создать группу (fire-and-forget). Результат — через событие `groups_changed`. */
    external fun groupCreate(sessionPtr: Long, name: String): Boolean
    /** Пригласить контакт в группу (по hex-ключу). */
    external fun groupInvite(sessionPtr: Long, gid: String, peerHex: String)
    /** Отправить текст в группу (fire-and-forget). UI добавляет оптимистично. */
    external fun groupSendText(sessionPtr: Long, gid: String, text: String)
    /** Список групп: JSON [{gid, name, members}]. */
    external fun groupList(sessionPtr: Long): String
    /** История группы: JSON [{text, mine, time}]. */
    external fun groupHistory(sessionPtr: Long, gid: String): String
    /** Ключи участников группы (hex): JSON-массив. */
    external fun groupMembers(sessionPtr: Long, gid: String): String

    // ── Комнаты-звонки (присутствие, Discord-style) ───────────────────────
    /** Войти в комнату gid: слать маячок присутствия с A/V-флагами. */
    external fun roomJoin(sessionPtr: Long, gid: String, audio: Boolean, video: Boolean)
    /** Выйти из комнаты gid. */
    external fun roomLeave(sessionPtr: Long, gid: String)
    /** Участники комнаты сейчас: JSON [{peer, audio, video}]. */
    external fun roomParticipants(sessionPtr: Long, gid: String): String
    /** Запустить мультипати аудио комнаты. JSON {sample_rate, channels, frame_len}. */
    external fun roomAudioStart(sessionPtr: Long, gid: String): String
    /** Остановить мультипати аудио комнаты. */
    external fun roomAudioStop(sessionPtr: Long)
    /** Захваченный PCM с микрофона → мультикаст всем в комнате. */
    external fun roomPushAudio(sessionPtr: Long, data: ShortArray, n: Int)
    /** Смешанный PCM комнаты для воспроизведения. Возвращает число сэмплов. */
    external fun roomPullAudio(sessionPtr: Long, out: ShortArray): Int
    external fun roomSetMuted(sessionPtr: Long, muted: Boolean)

    // ── Диагностика: тест пропускной способности меша ─────────────────────
    /** Включить приём бенча (считать входящие bench-датаграммы). */
    external fun benchRecvStart(sessionPtr: Long)
    /** Статистика приёмника: JSON {pkts, bytes, secs, mbps}. */
    external fun benchRecvStats(sessionPtr: Long): String
    /** Бластить total_kb КБ на пира. Возвращает JSON {sent_pkts, sent_bytes, secs, mbps}. */
    external fun benchBlast(sessionPtr: Long, peerHex: String, totalKb: Int): String
}
