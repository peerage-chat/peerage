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

    /** Инициализация. Возвращает JSON {pubkey, address, fingerprint} или {error}. */
    external fun start(dataDir: String): String

    /** Следующее событие как JSON-строка, либо "" если очередь пуста. */
    external fun pollEvent(): String

    /** Отправить текст. Возвращает id сообщения (>=0) или -1 при ошибке. */
    external fun sendText(keyHex: String, text: String): Long

    /** Держать фоновое соединение с пиром. */
    external fun connect(keyHex: String)

    /** Добавить контакт (TOFU). Возвращает "added" | "known" | "error". */
    external fun addContact(keyHex: String, name: String): String

    /** Контакты как JSON-массив. */
    external fun listContacts(): String

    /** История переписки с пиром как JSON-массив. */
    external fun history(keyHex: String): String

    external fun setVerified(keyHex: String, verified: Boolean)
    external fun renameContact(keyHex: String, name: String)
    external fun removeContact(keyHex: String)
    external fun isOnline(keyHex: String): Boolean
    external fun netPeers(): Int
    external fun netTree(): Int
    external fun stop()

    // ── Звонки (1:1) ──────────────────────────────────────────────────────
    /** Начать исходящий звонок. Возвращает JSON {call_id, sample_rate, channels, frame_len}. */
    external fun callStart(keyHex: String, video: Boolean, vw: Int, vh: Int): String
    /** Принять входящий звонок. Возвращает тот же JSON с аудио-параметрами. */
    external fun callAccept(keyHex: String, callId: Long, video: Boolean, vw: Int, vh: Int): String
    /** Завершить звонок. */
    external fun callEnd(keyHex: String, callId: Long)
    /** Захваченный PCM (первые n сэмплов) → отправка. */
    external fun callPushAudio(data: ShortArray, n: Int)
    /** Заполнить буфер PCM для воспроизведения. Возвращает число реальных сэмплов. */
    external fun callPullAudio(out: ShortArray): Int
    /** H.264 access-unit от MediaCodec-энкодера → фрагментация и отправка. */
    external fun callSendVideo(nal: ByteArray, keyframe: Boolean)
    /** Собранный входящий H.264 access-unit для MediaCodec-декодера (или пусто). */
    external fun callPullVideoPacket(): ByteArray
    external fun callSetMuted(muted: Boolean)

    // ── Группы (MLS) ──────────────────────────────────────────────────────
    /** Создать группу (fire-and-forget). Результат — через событие `groups_changed`
     *  + обновлённый кэш. Возвращает true, если команда принята. */
    external fun groupCreate(name: String): Boolean
    /** Пригласить контакт в группу (по hex-ключу). */
    external fun groupInvite(gid: String, peerHex: String)
    /** Отправить текст в группу (fire-and-forget). UI добавляет сообщение
     *  оптимистично; ядро обновит кэш истории после отправки. */
    external fun groupSendText(gid: String, text: String)
    /** Список групп: JSON [{gid, name, members}]. */
    external fun groupList(): String
    /** История группы: JSON [{text, mine, time}]. */
    external fun groupHistory(gid: String): String
    /** Ключи участников группы (hex): JSON-массив. */
    external fun groupMembers(gid: String): String

    // ── Комнаты-звонки (присутствие, Discord-style) ───────────────────────
    /** Войти в комнату gid: слать маячок присутствия с A/V-флагами. */
    external fun roomJoin(gid: String, audio: Boolean, video: Boolean)
    /** Выйти из комнаты gid. */
    external fun roomLeave(gid: String)
    /** Участники комнаты сейчас: JSON [{peer, audio, video}]. */
    external fun roomParticipants(gid: String): String
    /** Запустить мультипати аудио комнаты. JSON {sample_rate, channels, frame_len}. */
    external fun roomAudioStart(gid: String): String
    /** Остановить мультипати аудио комнаты. */
    external fun roomAudioStop()
    /** Захваченный PCM с микрофона → мультикаст всем в комнате. */
    external fun roomPushAudio(data: ShortArray, n: Int)
    /** Смешанный PCM комнаты для воспроизведения. Возвращает число сэмплов. */
    external fun roomPullAudio(out: ShortArray): Int
    external fun roomSetMuted(muted: Boolean)

    // ── Диагностика: тест пропускной способности меша ─────────────────────
    /** Включить приём бенча (считать входящие bench-датаграммы). */
    external fun benchRecvStart()
    /** Статистика приёмника: JSON {pkts, bytes, secs, mbps}. */
    external fun benchRecvStats(): String
    /** Бластить total_kb КБ на пира. Возвращает JSON {sent_pkts, sent_bytes, secs, mbps}. */
    external fun benchBlast(peerHex: String, totalKb: Int): String
}
