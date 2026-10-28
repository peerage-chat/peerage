package chat.peerage

/**
 * Мост к Rust-ядру (cdylib libpeerage_android.so). Имена методов совпадают с
 * JNI-символами `Java_chat_peerage_Native_*` в androidjni/src/lib.rs.
 *
 * libygg.so (Yggdrasil) грузим первой — libpeerage_android.so на неё ссылается.
 */
object Native {
    init {
        System.loadLibrary("c++_shared") // нужна libpeerage_android.so (C++ openh264)
        System.loadLibrary("ygg")
        System.loadLibrary("peerage_android")
    }

    /** Инициализация. vaultKey — 32-байтовый ключ из Android Keystore (VaultKeyManager).
     *  Возвращает JSON {pubkey, address, fingerprint, session_ptr} или {error}. */
    external fun start(dataDir: String, vaultKey: ByteArray): String

    /** Следующее событие как JSON-строка, либо "" если очередь пуста. */
    external fun pollEvent(sessionPtr: Long): String

    /** Секретная сид-фраза (24 слова BIP39) для резервной копии ключа (M4). СЕКРЕТ. */
    external fun seedPhrase(sessionPtr: Long): String

    /** Импорт личности из фразы (M4): валидирует и перезаписывает profile.json.
     *  true = успех (Kotlin затем перезапускает процесс). Не трогает живую сессию. */
    external fun importSeedPhrase(dataDir: String, phrase: String): Boolean

    /** Фраза корректна (24 слова с верной контрольной суммой). Без сессии. */
    external fun seedPhraseValid(phrase: String): Boolean

    /** Отложить нечитаемые данные в `quarantine/` (R-029). Без сессии. */
    external fun quarantineData(dataDir: String): Boolean

    /** Личность из фразы в чистый каталог, под ключом хранилища `vaultKey`. Без сессии:
     *  после карантина движок не поднят. */
    external fun restoreSeedPhrase(dataDir: String, vaultKey: ByteArray, phrase: String): Boolean

    /** Своё самоназвание (M2). "" если не задано. */
    external fun selfName(sessionPtr: Long): String

    /** Установить своё имя (M2): сохранить + разослать контактам (ProfileUpdate). */
    external fun setSelfName(sessionPtr: Long, name: String)

    /** Установить свой аватар (M2): scrub + контент-адресный блоб. Возвращает hex-id. */
    external fun setSelfAvatar(sessionPtr: Long, content: ByteArray, mime: String): String

    /** Байты своего аватара, либо null если не задан. */
    external fun selfAvatar(sessionPtr: Long): ByteArray?

    /** Отправить текст. Возвращает id сообщения (>=0) или -1 при ошибке. */
    external fun sendText(sessionPtr: Long, keyHex: String, text: String): Long

    /** Отправить медиа-вложение (M2): scrub + блоб + медиа-сообщение. id (>=0) или -1. */
    external fun sendMedia(sessionPtr: Long, keyHex: String, content: ByteArray, mime: String, name: String): Long

    /** То же + метаданные для карточки: длительность, размеры кадра, миниатюра (JPEG,
     *  можно пустой массив). Крупное вложение получатель качает по кнопке. */
    external fun sendMediaWithMeta(
        sessionPtr: Long,
        keyHex: String,
        content: ByteArray,
        mime: String,
        name: String,
        durationMs: Int,
        width: Int,
        height: Int,
        thumb: ByteArray,
        caption: String,
        /** Ответ вложением: id цитируемого сообщения (0 — обычное). */
        replyTo: Long,
        /** hex автора цитируемого сообщения (пусто — обычное). */
        replySenderHex: String,
    ): Long

    /** Отправить вложение ФАЙЛОМ: движок считает манифест потоково и отдаёт чанки
     *  с диска. Для крупного — единственный путь: гигабайт в память не читаем. */
    external fun sendMediaFile(
        sessionPtr: Long,
        peerHex: String,
        path: String,
        mime: String,
        name: String,
        durationMs: Int,
        width: Int,
        height: Int,
        thumb: ByteArray,
        caption: String,
        /** Ответ вложением: id цитируемого сообщения (0 — обычное). */
        replyTo: Long,
        /** hex автора цитируемого сообщения (пусто — обычное). */
        replySenderHex: String,
    ): Long

    /** Скачать вложение по кнопке. Готовность придёт событием `blob_received`. */
    external fun downloadMedia(sessionPtr: Long, peerHex: String, blobIdHex: String): Boolean

    /** Заметки себе — JSON-история чата с самим собой. */
    external fun notes(sessionPtr: Long): String

    /** Сохранить заметку себе. Возвращает id (или -1). */
    external fun saveNote(sessionPtr: Long, text: String): Long

    /** Удалить заметку по id. */
    external fun deleteNote(sessionPtr: Long, id: Long)

    /** Журнал звонков (JSON-массив, свежие первыми). */
    external fun callLog(sessionPtr: Long, limit: Int): String

    /** Очистить журнал звонков. */
    external fun clearCallLog(sessionPtr: Long)

    /** Визуальный отпечаток ключа (hex, 12 байт). Пустой ключ — свой отпечаток. */
    external fun visualFingerprint(sessionPtr: Long, keyHex: String): String

    /** Очередь отправки по получателям (JSON-массив). */
    external fun outboxByPeer(sessionPtr: Long): String

    /** Сбросить очередь всем. Возвращает число удалённых записей. */
    external fun clearOutboxAll(sessionPtr: Long): Int

    /** Заказанные докачки (JSON-массив hex-id) — переживают перезапуск. */
    external fun wantedBlobs(sessionPtr: Long): String

    /** Держать фоновое соединение с пиром. */
    external fun connect(sessionPtr: Long, keyHex: String)

    // ── Блобы (M2) ────────────────────────────────────────────────────────

    /** Контент-адрес (hex) содержимого — id блоба для ссылки. */
    external fun blobIdHex(sessionPtr: Long, content: ByteArray): String

    /** Прочитать сохранённый блоб по hex-id. Пустой массив, если нет. */
    external fun blobRead(sessionPtr: Long, idHex: String): ByteArray

    /** Есть ли блоб локально — без чтения байт (у крупного это гигабайт в память). */
    external fun blobExists(sessionPtr: Long, idHex: String): Boolean

    /** Расшифровать блоб в файл потоково (крупное вложение не идёт в память). */
    external fun blobExport(sessionPtr: Long, idHex: String, path: String): Boolean

    /** Добавить контакт по коду обмена: hex личности либо расширенная форма
     *  `личность:эпоха:адрес:подпись` (ADR-007). Возвращает "added"|"known"|"error". */
    external fun addContact(sessionPtr: Long, keyHex: String, name: String): String

    /** Текст в группу ответом на сообщение автора `replySender` (hex; `replyTo = 0` —
     *  не ответ). `mentionsJson` — `[{"at","len","key"}]`, смещения в байтах UTF-8. */
    external fun groupSendTextReply(
        sessionPtr: Long,
        gidHex: String,
        text: String,
        replyTo: Long,
        replySender: String,
        mentionsJson: String,
    )

    /** Удалить групповые сообщения у себя. `refsJson` = `[{"id":…,"sender":"hex"}]`. */
    external fun groupDeleteMessages(sessionPtr: Long, gidHex: String, refsJson: String)

    /** Отозвать свои групповые сообщения у участников. */
    external fun groupRetractMessages(sessionPtr: Long, gidHex: String, idsJson: String)

    /** Текст ответом на сообщение (`replyTo` = 0 — обычное). Возвращает id или -1. */
    external fun sendTextReply(sessionPtr: Long, keyHex: String, text: String, replyTo: Long, replyMine: Boolean): Long

    /** Удалить сообщения у себя. `refsJson` = `[{"id":…,"mine":…}]`. */
    external fun deleteMessages(sessionPtr: Long, keyHex: String, refsJson: String): Int

    /** Отозвать свои сообщения у собеседника. `idsJson` = `[…]`. */
    external fun retractMessages(sessionPtr: Long, keyHex: String, idsJson: String): Int

    /** Что и сколько войдёт в резервную копию данных (JSON BackupStatsDto). */
    external fun backupEstimate(sessionPtr: Long): String

    /** Создать копию в файл. Долгая операция — только с фонового потока. */
    external fun backupExport(sessionPtr: Long, path: String, partsJson: String, password: String): String

    /** Восстановить из копии. Возвращает JSON ДОБАВЛЕННОГО или {"error":...}. */
    external fun backupImport(sessionPtr: Long, path: String, password: String): String

    /** Ключ хранилища из пароля (Argon2id в ядре). Долгая операция — с фонового
     *  потока. Пустой массив при ошибке.
     *
     *  `profile` — номер профиля стоимости ИЗ ФАЙЛА (vault.meta): параметры KDF
     *  растут со временем, а ключ с другими параметрами — другой ключ. */
    external fun deriveVaultKey(password: String, salt: ByteArray, profile: Int): ByteArray

    /** Профиль стоимости для НОВЫХ файлов (источник истины — ядро). */
    external fun currentKdfProfile(): Int

    /** Свежая соль для KDF (16 байт). */
    external fun newVaultSalt(): ByteArray

    /** Кто сейчас флудит: JSON [{peer, dropped, pps, blocked}], по убыванию темпа. */
    external fun floodPeers(sessionPtr: Long): String

    /** Текущая эпоха адреса (0 — адрес совпадает с ключом личности). */
    external fun transportEpoch(sessionPtr: Long): Int

    /** Сменить адрес: пауза офлайн + подъём на новом ключе + рассылка объявления
     *  перечисленным контактам (hex через запятую). Долгая операция, fire-and-forget. */
    external fun rotateTransport(sessionPtr: Long, recipientsCsv: String, gapSecs: Int)

    /** Уйти в офлайн — перестать платить за чужой поток прямо сейчас. */
    external fun goOffline(sessionPtr: Long)

    /** Вернуться в сеть на текущем адресе. */
    external fun goOnline(sessionPtr: Long): Boolean

    /** Мы намеренно вне сети (кнопка «офлайн» или пауза смены адреса). */
    external fun isOffline(sessionPtr: Long): Boolean

    /** Свой код обмена для QR: hex личности, а после смены адреса — с подписанной
     *  привязкой к текущему адресу (иначе по коду до нас не достучатся). */
    external fun myContactCode(sessionPtr: Long): String

    /** Контакты как JSON-массив. */
    external fun listContacts(sessionPtr: Long): String

    /** Снимок офлайн-очереди: JSON {pending, bytes, peers, warn}. Ничего не удаляется
     *  само — по `warn` показываем предупреждение, решение за пользователем. */
    external fun outboxStats(sessionPtr: Long): String

    /** Сбросить очередь недоставленного пиру (явное действие). Возвращает удалённых. */
    external fun clearOutboxFor(sessionPtr: Long, keyHex: String): Int

    /** Очистить историю личного чата у себя: сообщения, закрепы, очередь отправки. */
    external fun clearDmHistory(sessionPtr: Long, keyHex: String)

    /** Что занимает место: JSON {blobs, blob_bytes, outbox_pending, outbox_bytes}. */
    external fun storageStats(sessionPtr: Long): String

    /** Удалить все сохранённые вложения. Возвращает освобождённые байты. */
    external fun clearBlobs(sessionPtr: Long): Long

    /**
     * Закрепить/открепить сообщение. `isGroup` выбирает вид чата, `senderHex` —
     * автор закрепляемого (ссылка на сообщение — пара «автор + id»).
     */
    external fun pinMessage(
        sessionPtr: Long,
        chatHex: String,
        isGroup: Boolean,
        msgId: Long,
        senderHex: String,
        on: Boolean,
    ): Int

    /**
     * Поиск по переписке. Пустой `chatHex` — искать во всех чатах.
     * Возвращает JSON `[{chat, group, ...сообщение}]`, свежие первыми.
     */
    external fun searchMessages(
        sessionPtr: Long,
        query: String,
        chatHex: String,
        isGroup: Boolean,
        limit: Int,
    ): String

    /** Исправить СВОЁ сообщение (текст или подпись вложения). 1 — получилось.
     *  `mentionsJson` — упоминания новой версии текста (только группа, иначе `[]`). */
    external fun editMessage(
        sessionPtr: Long,
        chatHex: String,
        isGroup: Boolean,
        msgId: Long,
        text: String,
        mentionsJson: String,
    ): Int

    /** Закреплённые сообщения чата: JSON-массив строк истории, свежие первыми. */
    external fun pinned(sessionPtr: Long, chatHex: String, isGroup: Boolean): String

    /**
     * Сводка по всем чатам для главной: JSON `[{chat, group, pinned_at, muted, last}]`,
     * где `last` — последнее сообщение чата (или null у пустого). Один вызов на
     * перерисовку списка: запрос на чат стоил бы тем дороже, чем больше переписок.
     */
    external fun chatSummaries(sessionPtr: Long): String

    /** Закрепить чат в списке / снять закреп. Пометка локальная, собеседнику не уходит. */
    external fun setChatPinned(sessionPtr: Long, chatHex: String, isGroup: Boolean, on: Boolean)

    /** Выключить/включить звук чата (тоже локально). */
    external fun setChatMuted(sessionPtr: Long, chatHex: String, isGroup: Boolean, muted: Boolean)

    /** Хвост переписки с пиром (последняя страница) как JSON-массив. */
    external fun history(sessionPtr: Long, keyHex: String): String

    /** Страница ВЫШЕ показанной: курсор — `time`/`row` самого верхнего сообщения. */
    external fun historyPage(sessionPtr: Long, keyHex: String, beforeTime: Long, beforeRow: Long): String

    /**
     * Переслать сообщения в другой чат. `srcGroup`/`dstGroup` — «это группа?»,
     * идентификаторы в hex (ключ собеседника или gid). `refsJson` — `[{id, sender}]`.
     * Возвращает, сколько сообщений ушло.
     */
    external fun forwardMessages(
        sessionPtr: Long,
        srcHex: String,
        srcGroup: Boolean,
        refsJson: String,
        dstHex: String,
        dstGroup: Boolean,
        hideAuthor: Boolean,
    ): Int

    external fun setVerified(sessionPtr: Long, keyHex: String, verified: Boolean)
    external fun renameContact(sessionPtr: Long, keyHex: String, name: String)
    external fun removeContact(sessionPtr: Long, keyHex: String)

    // ── Гейт контактов (запросы от незнакомцев + блок-лист) ────────────────
    /** Входящие запросы в контакты: JSON [{key, fingerprint, preview, time}]. */
    external fun contactRequests(sessionPtr: Long): String

    /** Одобрить запрос: незнакомец становится контактом. true при успехе. */
    external fun acceptContactRequest(sessionPtr: Long, keyHex: String): Boolean

    /** Отклонить запрос (тихо). true, если запрос был. */
    external fun declineContactRequest(sessionPtr: Long, keyHex: String): Boolean

    /** Заблокировать пира: будущие сообщения тихо дропаются. */
    external fun blockContact(sessionPtr: Long, keyHex: String): Boolean

    /** Разблокировать пира. */
    external fun unblockContact(sessionPtr: Long, keyHex: String): Boolean
    external fun isOnline(sessionPtr: Long, keyHex: String): Boolean
    external fun netPeers(sessionPtr: Long): Int
    external fun netTree(sessionPtr: Long): Int

    /** Снапшот стата сети: JSON {peer_count, tree, ping_ms, rx_rate, tx_rate, peers:[…]}. */
    external fun netStats(sessionPtr: Long): String
    external fun stop(sessionPtr: Long)

    /** Принудительный реконнект к публичным пирам (обход Yggdrasil backoff). */
    external fun reconnectPeers(sessionPtr: Long)

    // ── Группы (MLS) ──────────────────────────────────────────────────────
    /** Создать группу (fire-and-forget). Результат — через событие `groups_changed`. */
    external fun groupCreate(sessionPtr: Long, name: String): Boolean

    /** Пригласить контакт в группу (по hex-ключу) — карточкой-сообщением. */
    external fun groupInvite(sessionPtr: Long, gid: String, peerHex: String)

    /** Входящие приглашения в группы: JSON [{gid, name, inviter}]. */
    external fun groupInvites(sessionPtr: Long): String

    /** Принять приглашение в группу (карточка). Fire-and-forget. */
    external fun acceptGroupInvite(sessionPtr: Long, gid: String)

    /** Отклонить приглашение в группу. Fire-and-forget. */
    external fun declineGroupInvite(sessionPtr: Long, gid: String)

    /** Отправить текст в группу (fire-and-forget). UI добавляет оптимистично. */
    external fun groupSendText(sessionPtr: Long, gid: String, text: String)

    /** Отправить медиа-вложение в группу (M2): scrub + блоб всем + MLS-ссылка. */
    external fun groupSendMedia(
        sessionPtr: Long,
        gid: String,
        content: ByteArray,
        mime: String,
        name: String,
        durationMs: Int,
        width: Int,
        height: Int,
        thumb: ByteArray,
        caption: String,
        replyTo: Long,
        replySenderHex: String,
    )

    /** Отправить в группу вложение ФАЙЛОМ (потоково). */
    external fun groupSendMediaFile(
        sessionPtr: Long,
        gid: String,
        path: String,
        mime: String,
        name: String,
        durationMs: Int,
        width: Int,
        height: Int,
        thumb: ByteArray,
        caption: String,
        replyTo: Long,
        replySenderHex: String,
    )

    /** Отозвать участника группы по hex-ключу (ADR-004, только админ). */
    external fun groupRemoveMember(sessionPtr: Long, gid: String, peerHex: String)

    /** Ротация ключа группы (PCS, ADR-004). */
    external fun groupRekey(sessionPtr: Long, gid: String)

    /** Выйти из группы (ADR-004). */
    external fun groupLeave(sessionPtr: Long, gid: String)

    /** Список групп: JSON [{gid, name, members, admin}]. */
    external fun groupList(sessionPtr: Long): String

    /** Хвост истории группы (последняя страница): JSON [{text, mine, time, sender}]. */
    external fun groupHistory(sessionPtr: Long, gid: String): String

    /** Страница истории группы ВЫШЕ показанной; курсор — `time`/`row` верхнего. */
    external fun groupHistoryPage(sessionPtr: Long, gid: String, beforeTime: Long, beforeRow: Long): String

    /** Ключи участников группы (hex): JSON-массив. */
    external fun groupMembers(sessionPtr: Long, gid: String): String

    /** Снапшот присутствия группы: JSON {total, online} (агрегат считает движок). */
    external fun groupPresence(sessionPtr: Long, gid: String): String

    // ── Комнаты-звонки (присутствие, Discord-style) ───────────────────────
    /** DM-звонок: создать комнату, отправить RoomInvite пиру. Возвращает room_id hex. */
    external fun roomInviteDm(sessionPtr: Long, peerHex: String, audio: Boolean, video: Boolean): String

    /** Отклонить входящий DM-звонок. */
    external fun roomDeclineDm(sessionPtr: Long, peerHex: String)

    /** Принять DM-звонок: войти в комнату с регистрацией dm_peer. Возвращает room_id hex. */
    external fun roomJoinDm(sessionPtr: Long, peerHex: String, audio: Boolean, video: Boolean): String

    /** Позвать участников группы в её комнату; peersCsv — hex-ключи через запятую.
     *  Возвращает, скольким ушло приглашение. */
    external fun roomRing(sessionPtr: Long, gid: String, peersCsv: String, audio: Boolean, video: Boolean): Int

    /** Отклонить приглашение в комнату группы (звонящему ничего не уходит). */
    external fun roomDeclineGroup(sessionPtr: Long)

    /** Войти в комнату gid: слать маячок присутствия с A/V-флагами. */
    external fun roomJoin(sessionPtr: Long, gid: String, audio: Boolean, video: Boolean)

    /** Выйти из комнаты gid. */
    external fun roomLeave(sessionPtr: Long, gid: String)

    /** Участники комнаты сейчас: JSON [{peer, audio, video}]. */
    external fun roomParticipants(sessionPtr: Long, gid: String): String

    /** Живые участники любой комнаты по маячкам (не надо быть в ней самому). JSON [{peer, audio, video}]. */
    external fun roomPresencePeers(sessionPtr: Long, gidHex: String): String

    /** Запустить мультипати аудио комнаты. bitrate (бит/с, 0=дефолт) + music (Opus
     *  Audio-режим vs Voip) из настроек звука. JSON {sample_rate, channels, frame_len}. */
    external fun roomAudioStart(sessionPtr: Long, gid: String, bitrate: Int, music: Boolean): String

    /** Hex-ключи говорящих сейчас участников (active-speaker). JSON-массив. */
    external fun roomSpeakingPeers(sessionPtr: Long): String

    /** Сменить Opus-битрейт текущей аудио-сессии на лету (настройки звука без перезвона). */
    external fun roomSetAudioBitrate(sessionPtr: Long, bitrate: Int)

    /** Накопленные байты трафика по участникам (JSON {hex: rx, "self": tx}) для тех-инфо. */
    external fun roomTrafficJson(sessionPtr: Long): String

    /** Остановить мультипати аудио комнаты. */
    external fun roomAudioStop(sessionPtr: Long)

    /** Захваченный PCM с микрофона → мультикаст всем в комнате. */
    external fun roomPushAudio(sessionPtr: Long, data: ShortArray, n: Int)

    /** Смешанный PCM комнаты для воспроизведения. Возвращает число сэмплов. */
    external fun roomPullAudio(sessionPtr: Long, out: ShortArray): Int
    external fun roomSetMuted(sessionPtr: Long, muted: Boolean)

    /**
     * Закодированный H264 access-unit → фрагментация + мультикаст всем в комнате.
     * [rotation] — угол сенсора / 90 (приёмник доворачивает), [fps] — темп вывода у
     * приёмника; оба едут в протоколе LiteRTC. `true` — участник просит ключевой
     * кадр, кодеру выдать его следующим.
     */
    external fun roomPushVideo(sessionPtr: Long, data: ByteArray, keyframe: Boolean, rotation: Int, fps: Int): Boolean

    /** Потолок битрейта видео из настроек качества: выше оценка полосы не идёт. */
    external fun roomSetVideoBitrate(sessionPtr: Long, bps: Int)

    /** Сколько сейчас отдать видео-кодеру, бит/с, по оценке полосы (0 — звонка нет). */
    external fun roomVideoTargetBitrate(sessionPtr: Long): Int

    /** Декодер видео [peerHex] начат заново — попросить у него ключевой кадр. */
    external fun roomRequestKeyframe(sessionPtr: Long, peerHex: String)

    /**
     * Следующий входящий H264-кадр от peer, если его пора декодировать:
     * `[поворот (угол/90)][срок кадра от «сейчас», мкс: i32 BE, ≤ 0][access-unit…]`.
     * Пустой массив — пока нечего (сроки расписывает LiteRTC по меткам захвата).
     */
    external fun roomPullVideoPacket(sessionPtr: Long, peerHex: String): ByteArray

    // ── Диагностика: тест пропускной способности меша ─────────────────────
    /** Включить приём бенча (считать входящие bench-датаграммы). */
    external fun benchRecvStart(sessionPtr: Long)

    /** Статистика приёмника: JSON {pkts, bytes, secs, mbps}. */
    external fun benchRecvStats(sessionPtr: Long): String

    /** Бластить total_kb КБ на пира. Возвращает JSON {sent_pkts, sent_bytes, secs, mbps}. */
    external fun benchBlast(sessionPtr: Long, peerHex: String, totalKb: Int): String
}
