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
}
