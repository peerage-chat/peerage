package dev.p2p.messenger

/**
 * Мост к Rust-ядру (cdylib libp2p_android.so). Имена методов совпадают с
 * JNI-символами `Java_dev_p2p_messenger_Native_*` в androidjni/src/lib.rs.
 *
 * libygg.so (Yggdrasil) грузим первой — libp2p_android.so на неё ссылается.
 */
object Native {
    init {
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
    /** RGB8-кадр с камеры/экрана → отправка видео. */
    external fun callPushVideo(rgb: ByteArray)
    /** Входящий видео-кадр: [w:4 LE][h:4 LE][rgb…] или пусто. */
    external fun callPullVideo(): ByteArray
    external fun callSetMuted(muted: Boolean)
}
