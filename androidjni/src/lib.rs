//! JNI-кодек Android ↔ движок (`p2p-engine`). Грузится из Kotlin:
//! `System.loadLibrary("p2p_android")`.
//!
//! Этот крейт — ТОНКИЙ слой маршалинга (ADR-003): декодирует JNI-аргументы
//! (JString/byte[] → ключи/строки), зовёт метод [`p2p_engine::Engine`] и кодирует
//! результат обратно. Домейн-логики здесь нет — она в движке, который
//! юнит-тестируется без устройства. Указатель сессии (`session_ptr`) — это
//! `Box<Arc<Engine>>`, переданный в Kotlin как jlong.
//!
//! Модель событий — поллинг: Kotlin периодически зовёт `pollEvent()`.

mod bench_jni;
mod call_jni;
mod group_jni;

use std::path::PathBuf;
use std::sync::Arc;

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use p2p_engine::{parse_key, Engine};

// Ловим Rust-паники на JNI-границе: паника сквозь FFI = UB (на Android — SIGABRT
// без стека). catch_unwind превращает её в ошибку с логом и безопасным дефолтом.
#[macro_export]
macro_rules! jni_safe {
    ($default:expr, $body:block) => {
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
            Ok(v) => v,
            Err(e) => {
                let msg = e.downcast_ref::<&str>().copied()
                    .or_else(|| e.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unknown panic");
                ::log::error!("JNI panic: {msg}");
                $default
            }
        }
    };
}

// ── Хелперы маршалинга строк ────────────────────────────────────────────────
pub(crate) fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(|v| v.into()).unwrap_or_default()
}
pub(crate) fn ret(env: &JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|v| v.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Разыменовать `session_ptr` в `&Arc<Engine>` (или None, если 0).
pub(crate) unsafe fn engine<'a>(session_ptr: jlong) -> Option<&'a Arc<Engine>> {
    if session_ptr == 0 { None } else { Some(&*(session_ptr as *const Arc<Engine>)) }
}

// ── Жизненный цикл ──────────────────────────────────────────────────────────

/// Инициализация. dataDir — приватный каталог приложения (filesDir).
/// Возвращает JSON `{session_ptr, pubkey, address, fingerprint}` или `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    vault_key: JByteArray<'local>,
) -> jstring {
    let _ = android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("p2p"),
    );

    let dir = PathBuf::from(jstr(&mut env, &data_dir));

    // Ключ at-rest из Android Keystore (VaultKeyManager в Kotlin разворачивает его
    // и передаёт 32 байта plaintext). Неверный размер → at-rest отключено.
    let key: Option<[u8; 32]> = match env.convert_byte_array(&vault_key) {
        Ok(raw) if raw.len() == 32 => Some(std::array::from_fn(|i| raw[i])),
        Ok(raw) => { log::warn!("vault key: неверный размер {} (ожидается 32); at-rest отключено", raw.len()); None }
        Err(e) => { log::warn!("vault key: ошибка чтения JByteArray ({e}); at-rest отключено"); None }
    };

    match Engine::start(dir, key) {
        Ok(ok) => {
            let session_ptr = Box::into_raw(Box::new(ok.engine)) as jlong;
            let json = serde_json::json!({
                "session_ptr": session_ptr,
                "pubkey": hex::encode(ok.pubkey),
                "address": ok.address,
                "fingerprint": p2p_core::identity::fingerprint_words(&ok.pubkey).join(" "),
            });
            ret(&env, &json.to_string())
        }
        Err(e) => ret(&env, &serde_json::json!({"error": e}).to_string()),
    }
}

/// Освободить сессию (выгружает Arc<Engine>).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_destroySession<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    if session_ptr != 0 {
        let _boxed = unsafe { Box::from_raw(session_ptr as *mut Arc<Engine>) };
    }
}

/// Следующее событие как JSON-строка, либо "" если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_pollEvent<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        ret(&env, &e.poll_event().unwrap_or_default())
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_stop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.shutdown(); }
    });
}

/// Секретная сид-фраза (24 слова BIP39) для резервной копии ключа (M4). СЕКРЕТ —
/// показывать только на FLAG_SECURE-экране. "" если сессии нет.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_seedPhrase<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        ret(&env, &e.seed_phrase())
    })
}

/// Импорт личности из сид-фразы (M4 «У меня уже есть ключ»): валидирует 24 слова и
/// ПЕРЕЗАПИСЫВАЕТ profile.json импортированной личностью (запечатывается глобальным
/// vault-ключом, уже установленным при старте движка). Возвращает true при успехе.
/// Kotlin после успеха перезапускает процесс — новый profile.json грузится начисто.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_importSeedPhrase<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    phrase: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let dir = jstr(&mut env, &data_dir);
        let phrase = jstr(&mut env, &phrase);
        match p2p_core::Identity::from_phrase(&phrase) {
            Ok(identity) => {
                let path = std::path::Path::new(&dir).join("profile.json");
                match (p2p_core::store::Profile { identity }).save(&path) {
                    Ok(()) => JNI_TRUE,
                    Err(_) => JNI_FALSE,
                }
            }
            Err(_) => JNI_FALSE,
        }
    })
}

/// Своё самоназвание (M2). "" если не задано.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_selfName<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        ret(&env, &e.self_name())
    })
}

/// Установить своё имя (M2): сохранить и разослать контактам (ProfileUpdate).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_setSelfName<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    name: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        e.set_self_name(&jstr(&mut env, &name));
    });
}

/// Установить свой аватар (M2): scrub + сохранить блобом. Возвращает hex-id ("" при ошибке).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_setSelfAvatar<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    content: JByteArray<'local>,
    mime: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        let mime = jstr(&mut env, &mime);
        ret(&env, &e.set_self_avatar(bytes, &mime))
    })
}

/// Байты своего аватара (M2), либо null если не задан.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_selfAvatar<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jbyteArray {
    jni_safe!(std::ptr::null_mut(), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return std::ptr::null_mut() };
        match e.self_avatar() {
            Some(bytes) => env.byte_array_from_slice(&bytes).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut()),
            None => std::ptr::null_mut(),
        }
    })
}

// ── Сообщения 1:1 ───────────────────────────────────────────────────────────

/// Отправить текст. Возвращает id (≥0) либо -1 при ошибке.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_sendText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    text: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let text = jstr(&mut env, &text);
        e.send_text(&key, &text).unwrap_or(-1)
    })
}

/// Отправить медиа-вложение (M2): scrub картинок + блоб + медиа-сообщение.
/// Возвращает id (≥0) либо -1 при ошибке.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_sendMedia<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    content: JByteArray<'local>,
    mime: JString<'local>,
    name: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        if bytes.is_empty() { return -1; }
        let mime = jstr(&mut env, &mime);
        let name = jstr(&mut env, &name);
        e.send_media(&key, bytes, &mime, &name).unwrap_or(-1)
    })
}

/// Инициировать/держать соединение с пиром (фоновый коннект).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_connect<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) { e.connect(key); }
    });
}

/// История переписки с пиром как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_history<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "[]") };
        ret(&env, &e.history_json(&key))
    })
}

// ── Блобы (M2) ──────────────────────────────────────────────────────────────

/// Предложить блоб пиру (картинка/файл/голосовое). Fire-and-forget.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_blobOffer<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
    content: JByteArray<'local>,
    mime: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        if bytes.is_empty() { return; }
        e.blob_offer(peer, bytes, jstr(&mut env, &mime));
    });
}

/// Контент-адрес (hex) содержимого — id блоба для ссылки из сообщения/аватарки.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_blobIdHex<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    content: JByteArray<'local>,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        ret(&env, &e.blob_id_hex(&bytes))
    })
}

/// Прочитать сохранённый блоб по hex-id. Пустой массив, если блоба нет.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_blobRead<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    id_hex: JString<'local>,
) -> JByteArray<'local> {
    let empty = env.new_byte_array(0).unwrap();
    let Some(e) = (unsafe { engine(session_ptr) }) else { return empty };
    let id = jstr(&mut env, &id_hex);
    match e.blob_read(&id) {
        Some(bytes) => env.byte_array_from_slice(&bytes).unwrap_or(empty),
        None => empty,
    }
}

// ── Контакты (TOFU) ─────────────────────────────────────────────────────────

/// Добавить контакт. Возвращает "added" | "known" | "error".
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_addContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    name: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "error"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "error") };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "error") };
        let name = jstr(&mut env, &name);
        ret(&env, e.add_contact(&key, &name))
    })
}

/// Снимок офлайн-очереди: JSON {pending, bytes, peers, warn}. По `warn` клиент
/// показывает предупреждение о разросшейся очереди (ничего не удаляется само).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_outboxStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.outbox_stats_json())
    })
}

/// Сбросить очередь недоставленного пиру (явное действие пользователя).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_clearOutboxFor<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        parse_key(&jstr(&mut env, &key_hex)).map(|k| e.clear_outbox_for(&k) as jint).unwrap_or(0)
    })
}

/// Список контактов как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_listContacts<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.contacts_json())
    })
}

/// Пометить контакт проверенным/непроверенным.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_setVerified<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    verified: jboolean,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            e.set_verified(&key, verified != JNI_FALSE);
        }
    });
}

/// Переименовать контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_renameContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    name: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) {
            e.rename_contact(&key, &jstr(&mut env, &name));
        }
    });
}

/// Удалить контакт.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_removeContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(key) = parse_key(&jstr(&mut env, &key_hex)) { e.remove_contact(&key); }
    });
}

// ── Гейт контактов (запросы от незнакомцев + блок-лист) ──────────────────────

/// Входящие запросы в контакты как JSON-массив [{key, fingerprint, preview, time}].
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_contactRequests<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.contact_requests_json())
    })
}

/// Одобрить запрос: незнакомец → контакт. Возвращает true при успехе.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_acceptContactRequest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let ok = parse_key(&jstr(&mut env, &key_hex)).map(|k| e.accept_contact_request(&k)).unwrap_or(false);
        if ok { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Отклонить запрос (тихо). Возвращает true, если запрос был.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_declineContactRequest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let ok = parse_key(&jstr(&mut env, &key_hex)).map(|k| e.decline_contact_request(&k)).unwrap_or(false);
        if ok { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Заблокировать пира: будущие сообщения тихо дропаются, запрос снят, из контактов убран.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_blockContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let ok = parse_key(&jstr(&mut env, &key_hex)).map(|k| e.block_contact(&k)).unwrap_or(false);
        if ok { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Разблокировать пира. Возвращает true, если был в блок-листе.
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_unblockContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let ok = parse_key(&jstr(&mut env, &key_hex)).map(|k| e.unblock_contact(&k)).unwrap_or(false);
        if ok { JNI_TRUE } else { JNI_FALSE }
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_isOnline<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let online = parse_key(&jstr(&mut env, &key_hex)).map(|k| e.is_online(&k)).unwrap_or(false);
        if online { JNI_TRUE } else { JNI_FALSE }
    })
}

// ── Сеть ────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        unsafe { engine(session_ptr) }.map(|e| e.net_peers()).unwrap_or(0)
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netTree<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        unsafe { engine(session_ptr) }.map(|e| e.net_tree()).unwrap_or(0)
    })
}

/// Снапшот стата сети: JSON NetStats (агрегаты считает движок).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_netStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &serde_json::to_string(&e.net_stats()).unwrap_or_else(|_| "{}".into()))
    })
}

/// Принудительно переподключить публичных Yggdrasil-пиров (обход backoff).
#[no_mangle]
pub extern "system" fn Java_dev_p2p_messenger_Native_reconnectPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.reconnect_peers(); }
    });
}
