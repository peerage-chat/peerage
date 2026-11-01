//! JNI-кодек Android ↔ движок (`peerage-engine`). Грузится из Kotlin:
//! `System.loadLibrary("peerage_android")`.
//!
//! Этот крейт — ТОНКИЙ слой маршалинга (ADR-003): декодирует JNI-аргументы
//! (JString/byte[] → ключи/строки), зовёт метод [`peerage_engine::Engine`] и кодирует
//! результат обратно. Домейн-логики здесь нет — она в движке, который
//! юнит-тестируется без устройства. Указатель сессии (`session_ptr`) — это
//! `Box<Arc<Engine>>`, переданный в Kotlin как jlong. Сессия живёт до конца
//! процесса и не освобождается: её указатель держат фоновые потоки Kotlin
//! (`pollThread`, `ops`), и освобождение при живых потоках было бы use-after-free.
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

use peerage_engine::{parse_key, Engine};

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
/// Пустой `byte[]` — дефолт для функций, возвращающих массив. Если JVM не смогла
/// выделить даже его (OOM / висящее исключение), отдаём null, а не паникуем.
pub(crate) fn empty_bytes<'a>(env: &JNIEnv<'a>) -> JByteArray<'a> {
    env.new_byte_array(0).unwrap_or_default()
}

/// Разыменовать `session_ptr` в `&Arc<Engine>` (или None, если 0).
pub(crate) unsafe fn engine<'a>(session_ptr: jlong) -> Option<&'a Arc<Engine>> {
    if session_ptr == 0 { None } else { Some(&*(session_ptr as *const Arc<Engine>)) }
}

// ── Жизненный цикл ──────────────────────────────────────────────────────────

/// Инициализация. dataDir — приватный каталог приложения (filesDir).
/// Возвращает JSON `{session_ptr, pubkey, address, fingerprint}` или `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    vault_key: JByteArray<'local>,
) -> jstring {
    jni_safe!(ret(&env, r#"{"error":"panic in start"}"#), {
        let _ = android_logger::init_once(
            android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("core"),
        );
        // Паника в фоновом потоке ядра (звонок, мессенджер) пишет в stderr, а его на
        // Android никто не читает: поток молча умирает, и остаётся гадать, почему
        // встал звонок. Пусть она будет в logcat. Для паник на границе JNI это
        // не замена catch_unwind — они так же перехватываются, просто видны.
        std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

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
                    "fingerprint": peerage_core::identity::fingerprint_words(&ok.pubkey).join(" "),
                });
                ret(&env, &json.to_string())
            }
            Err(e) => ret(&env, &serde_json::json!({"error": e}).to_string()),
        }
    })
}

/// Следующее событие как JSON-строка, либо "" если очередь пуста.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_pollEvent<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_stop<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_seedPhrase<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        ret(&env, &e.seed_phrase())
    })
}

/// Проверка копии ключа: `picks` — пары «номер слово» через пробел («4 grain 11 quote»),
/// номер с 1. true — все слова на своих местах; кривой ввод — false. Фраза в Kotlin не
/// попадает: экран проверки знает только ответ.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_checkSeedWords<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    picks: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let picks = jstr(&mut env, &picks);
        let parts: Vec<&str> = picks.split_whitespace().collect();
        if parts.len() % 2 != 0 { return JNI_FALSE; }
        let mut pairs = Vec::with_capacity(parts.len() / 2);
        for pair in parts.chunks(2) {
            let Ok(n) = pair[0].parse::<usize>() else { return JNI_FALSE };
            pairs.push((n, pair[1]));
        }
        if e.seed_words_match(&pairs) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Словарь BIP39 (2048 слов через пробел) — подсказки при вводе фразы. Без сессии.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_bip39Words<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    jni_safe!(ret(&env, ""), { ret(&env, &peerage_core::identity::bip39_words().join(" ")) })
}

/// Импорт личности из сид-фразы (M4 «У меня уже есть ключ»): валидирует 24 слова и
/// ПЕРЕЗАПИСЫВАЕТ profile.json импортированной личностью (запечатывается глобальным
/// vault-ключом, уже установленным при старте движка). Возвращает true при успехе.
/// Kotlin после успеха перезапускает процесс — новый profile.json грузится начисто.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_importSeedPhrase<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    phrase: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let dir = jstr(&mut env, &data_dir);
        let phrase = jstr(&mut env, &phrase);
        match peerage_core::Identity::from_phrase(&phrase) {
            Ok(identity) => {
                let path = std::path::Path::new(&dir).join("profile.json");
                match (peerage_core::store::Profile { identity }).save(&path) {
                    Ok(()) => JNI_TRUE,
                    Err(_) => JNI_FALSE,
                }
            }
            Err(_) => JNI_FALSE,
        }
    })
}

/// Сид-фраза корректна (24 слова BIP39 с верной контрольной суммой). Без сессии:
/// экран восстановления проверяет фразу ДО того, как что-либо трогать на диске.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_seedPhraseValid<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    phrase: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let phrase = jstr(&mut env, &phrase);
        if peerage_core::Identity::from_phrase(&phrase).is_ok() { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Отложить нечитаемые данные в `quarantine/` (R-029): ключ хранилища не подошёл,
/// и человек выбрал «восстановить» или «начать заново». Без сессии — движок в
/// этом состоянии не поднят. true — данные отложены.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_quarantineData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let dir = PathBuf::from(jstr(&mut env, &data_dir));
        match peerage_engine::quarantine_user_data(&dir) {
            Ok(dst) => {
                log::warn!("данные отложены в {}", dst.display());
                JNI_TRUE
            }
            Err(e) => {
                log::error!("карантин данных не удался: {e}");
                JNI_FALSE
            }
        }
    })
}

/// Восстановить личность из сид-фразы в чистом каталоге (после карантина): профиль
/// запечатывается ключом хранилища `vault_key`. Нужен отдельно от
/// [`importSeedPhrase`]: движок здесь не поднят, и ключ хранилища ещё не задан.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_restoreSeedPhrase<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    vault_key: JByteArray<'local>,
    phrase: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let dir = jstr(&mut env, &data_dir);
        let phrase = jstr(&mut env, &phrase);
        let Ok(raw) = env.convert_byte_array(&vault_key) else { return JNI_FALSE };
        let Ok(key) = <[u8; 32]>::try_from(raw.as_slice()) else { return JNI_FALSE };
        // Ключ задаётся на процесс один раз. Если он уже задан ДРУГИМ, профиль лёг
        // бы под чужой ключ и не открылся бы на следующем старте — отказываемся.
        peerage_core::vault::init_with_key(key);
        if peerage_core::vault::key_bytes() != Some(key) {
            return JNI_FALSE;
        }
        let Ok(identity) = peerage_core::Identity::from_phrase(&phrase) else { return JNI_FALSE };
        let path = std::path::Path::new(&dir).join("profile.json");
        if (peerage_core::store::Profile { identity }).save(&path).is_ok() { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Своё самоназвание (M2). "" если не задано.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_selfName<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_setSelfName<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_setSelfAvatar<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_selfAvatar<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_sendText<'local>(
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

/// Отправить текст ОТВЕТОМ на сообщение `replyTo` (`replyMine` — чьё оно у нас).
/// `replyTo = 0` — обычное сообщение. Возвращает id (≥0) либо -1.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_sendTextReply<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    text: JString<'local>,
    reply_to: jlong,
    reply_mine: jboolean,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let text = jstr(&mut env, &text);
        e.send_text_reply(&key, &text, reply_to.max(0) as u64, reply_mine != 0).unwrap_or(-1)
    })
}

/// Удалить сообщения ТОЛЬКО У СЕБЯ. `refsJson` — `[{"id":123,"mine":true}, …]`.
/// Возвращает число удалённых.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_deleteMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    refs_json: JString<'local>,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return 0 };
        // Разбираем через Value, а не derive: serde как прямая зависимость этому
        // крейту-маршалингу не нужна, а форма тут ровно одна.
        let raw: serde_json::Value =
            serde_json::from_str(&jstr(&mut env, &refs_json)).unwrap_or(serde_json::Value::Null);
        let refs: Vec<(u64, bool)> = raw
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| Some((v["id"].as_u64()?, v["mine"].as_bool().unwrap_or(false))))
                    .collect()
            })
            .unwrap_or_default();
        e.delete_messages(&key, &refs) as jint
    })
}

/// Отозвать СВОИ сообщения у собеседника («удалить у всех»). `idsJson` — `[123, …]`.
/// Возвращает число отправленных просьб.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_retractMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    ids_json: JString<'local>,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return 0 };
        let ids: Vec<u64> = serde_json::from_str(&jstr(&mut env, &ids_json)).unwrap_or_default();
        e.retract_messages(&key, &ids) as jint
    })
}

/// Отправить медиа-вложение (M2): scrub картинок + блоб + медиа-сообщение.
/// Возвращает id (≥0) либо -1 при ошибке.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_sendMedia<'local>(
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

/// Отправить вложение с метаданными для карточки: длительность, размеры кадра и
/// миниатюра (JPEG-байты, можно пустой массив). Их извлекает клиент — движок сам
/// декодировать медиа не умеет.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_chat_peerage_Native_sendMediaWithMeta<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    content: JByteArray<'local>,
    mime: JString<'local>,
    name: JString<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: JByteArray<'local>,
    caption: JString<'local>,
    reply_to: jlong,
    reply_sender_hex: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let bytes = env.convert_byte_array(&content).unwrap_or_default();
        if bytes.is_empty() { return -1; }
        let mime = jstr(&mut env, &mime);
        let name = jstr(&mut env, &name);
        let caption = jstr(&mut env, &caption);
        let thumb_bytes = env.convert_byte_array(&thumb).unwrap_or_default();
        let info = peerage_engine::MediaInfo {
            fwd_from: String::new(),
            fwd_anon: false,
            duration_ms: duration_ms.max(0) as u32,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            thumb: (!thumb_bytes.is_empty()).then_some(thumb_bytes),
            caption,
            reply_to: reply_to.max(0) as u64,
            reply_sender: jstr(&mut env, &reply_sender_hex),
        };
        e.send_media_with(&key, bytes, &mime, &name, info).unwrap_or(-1)
    })
}

/// Отправить вложение ФАЙЛОМ: движок считает манифест потоково и отдаёт чанки с
/// диска. Для крупного это единственный путь — читать гигабайт в память нельзя.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_chat_peerage_Native_sendMediaFile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    path: JString<'local>,
    mime: JString<'local>,
    name: JString<'local>,
    duration_ms: jint,
    width: jint,
    height: jint,
    thumb: JByteArray<'local>,
    caption: JString<'local>,
    reply_to: jlong,
    reply_sender_hex: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return -1 };
        let path = jstr(&mut env, &path);
        let mime = jstr(&mut env, &mime);
        let name = jstr(&mut env, &name);
        let caption = jstr(&mut env, &caption);
        let thumb_bytes = env.convert_byte_array(&thumb).unwrap_or_default();
        let info = peerage_engine::MediaInfo {
            fwd_from: String::new(),
            fwd_anon: false,
            duration_ms: duration_ms.max(0) as u32,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            thumb: (!thumb_bytes.is_empty()).then_some(thumb_bytes),
            caption,
            reply_to: reply_to.max(0) as u64,
            reply_sender: jstr(&mut env, &reply_sender_hex),
        };
        e.send_media_file(&key, std::path::Path::new(&path), &mime, &name, info).unwrap_or(-1)
    })
}

/// Скачать вложение по кнопке (крупное не тянется автоматически). Готовность придёт
/// событием `blob_received`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_downloadMedia<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    peer_hex: JString<'local>,
    blob_id_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let Some(peer) = parse_key(&jstr(&mut env, &peer_hex)) else { return JNI_FALSE };
        let id = jstr(&mut env, &blob_id_hex);
        if e.download_media(&peer, &id) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Заказанные пользователем докачки (hex-id) — переживают перезапуск.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_wantedBlobs<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.wanted_json())
    })
}

/// Инициировать/держать соединение с пиром (фоновый коннект).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_connect<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_history<'local>(
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

/// Страница истории ВЫШЕ уже показанной: курсор — `time`/`row` самого верхнего
/// сообщения. Открытие чата отдаёт только хвост, остальное клиент просит сам.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_historyPage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    before_time: jlong,
    before_row: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Some(key) = parse_key(&jstr(&mut env, &key_hex)) else { return ret(&env, "[]") };
        ret(&env, &e.history_page_json(&key, before_time, before_row))
    })
}

/// Переслать сообщения в другой чат. `src_group`/`dst_group` — «это группа?»,
/// идентификатор в hex (ключ пира или gid). Возвращает, сколько ушло.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_forwardMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    src_hex: JString<'local>,
    src_group: jboolean,
    refs_json: JString<'local>,
    dst_hex: JString<'local>,
    dst_group: jboolean,
    hide_author: jboolean,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let src = peerage_engine::ChatKey::parse(&jstr(&mut env, &src_hex), src_group != 0);
        let dst = peerage_engine::ChatKey::parse(&jstr(&mut env, &dst_hex), dst_group != 0);
        let (Some(src), Some(dst)) = (src, dst) else { return 0 };
        let raw: serde_json::Value =
            serde_json::from_str(&jstr(&mut env, &refs_json)).unwrap_or(serde_json::Value::Null);
        let refs: Vec<(u64, String)> = raw
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some((v["id"].as_u64()?, v["sender"].as_str().unwrap_or("").to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        e.forward_messages(src, &refs, dst, hide_author != 0) as jint
    })
}

/// Исправить своё сообщение (текст или подпись вложения). Возвращает 1/0.
/// `mentionsJson` — `[{"at","len","key"}]` в байтах UTF-8 (только группа).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_editMessage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    chat_hex: JString<'local>,
    is_group: jboolean,
    msg_id: jlong,
    text: JString<'local>,
    mentions_json: JString<'local>,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let Some(chat) = peerage_engine::ChatKey::parse(&jstr(&mut env, &chat_hex), is_group != 0)
        else {
            return 0;
        };
        let text = jstr(&mut env, &text);
        let mentions = parse_mentions(&jstr(&mut env, &mentions_json));
        i32::from(e.edit_message_with(chat, msg_id as u64, &text, &mentions))
    })
}

/// Упоминания от клиента. Кривой JSON — не повод терять сообщение: уходит без
/// упоминаний (участки всё равно проверяет `peerage_core::mention::clean`).
pub(crate) fn parse_mentions(json: &str) -> Vec<peerage_engine::Mention> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Закрепить/открепить сообщение в чате (личном или групповом). Возвращает 1/0.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_pinMessage<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    chat_hex: JString<'local>,
    is_group: jboolean,
    msg_id: jlong,
    sender_hex: JString<'local>,
    on: jboolean,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        let Some(chat) = peerage_engine::ChatKey::parse(&jstr(&mut env, &chat_hex), is_group != 0)
        else {
            return 0;
        };
        let sender = jstr(&mut env, &sender_hex);
        i32::from(e.pin_message(chat, msg_id as u64, &sender, on != 0))
    })
}

/// Закреплённые сообщения чата: JSON-массив строк истории, свежие первыми.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_pinned<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    chat_hex: JString<'local>,
    is_group: jboolean,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let Some(chat) = peerage_engine::ChatKey::parse(&jstr(&mut env, &chat_hex), is_group != 0)
        else {
            return ret(&env, "[]");
        };
        ret(&env, &e.pinned_json(chat))
    })
}

/// Сводка по всем чатам для главной: JSON `[{chat, group, pinned_at, muted, last}]`.
/// Один вызов на перерисовку списка вместо запроса на чат.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_chatSummaries<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.chat_summaries_json())
    })
}

/// Закрепить чат в списке или снять закреп (пометка локальная, не уходит собеседнику).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_setChatPinned<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    chat_hex: JString<'local>,
    is_group: jboolean,
    on: jboolean,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Some(chat) = peerage_engine::ChatKey::parse(&jstr(&mut env, &chat_hex), is_group != 0)
        else {
            return;
        };
        e.set_chat_pinned(chat, on != 0);
    })
}

/// Выключить/включить звук чата (тоже локально).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_setChatMuted<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    chat_hex: JString<'local>,
    is_group: jboolean,
    muted: jboolean,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let Some(chat) = peerage_engine::ChatKey::parse(&jstr(&mut env, &chat_hex), is_group != 0)
        else {
            return;
        };
        e.set_chat_muted(chat, muted != 0);
    })
}

/// Поиск по переписке. `chat_hex` пустой — искать во ВСЕХ чатах.
/// Возвращает JSON `[{chat, group, ...сообщение}]`, свежие первыми.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_searchMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    query: JString<'local>,
    chat_hex: JString<'local>,
    is_group: jboolean,
    limit: jint,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        let q = jstr(&mut env, &query);
        let chat_str = jstr(&mut env, &chat_hex);
        let chat = if chat_str.trim().is_empty() {
            None
        } else {
            peerage_engine::ChatKey::parse(&chat_str, is_group != 0)
        };
        ret(&env, &e.search_json(&q, chat, limit.max(0) as usize))
    })
}

// ── Блобы (M2) ──────────────────────────────────────────────────────────────

/// Контент-адрес (hex) содержимого — id блоба для ссылки из сообщения/аватарки.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_blobIdHex<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_blobRead<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    id_hex: JString<'local>,
) -> JByteArray<'local> {
    jni_safe!(empty_bytes(&env), {
        let empty = empty_bytes(&env);
        let Some(e) = (unsafe { engine(session_ptr) }) else { return empty };
        let id = jstr(&mut env, &id_hex);
        match e.blob_read(&id) {
            Some(bytes) => env.byte_array_from_slice(&bytes).unwrap_or(empty),
            None => empty,
        }
    })
}

/// Есть ли блоб локально. Нужен, чтобы понять «скачано ли вложение», не читая его
/// байты: у крупного файла такая проверка стоила бы гигабайта памяти.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_blobExists<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    id_hex: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let id = jstr(&mut env, &id_hex);
        if e.blob_exists(&id) { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Расшифровать блоб в файл по пути `path` потоково — для проигрывания и открытия
/// крупных вложений без чтения их в память. `true` — файл записан целиком.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_blobExport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    id_hex: JString<'local>,
    path: JString<'local>,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        let id = jstr(&mut env, &id_hex);
        let dst = jstr(&mut env, &path);
        if e.blob_export(&id, std::path::Path::new(&dst)) { JNI_TRUE } else { JNI_FALSE }
    })
}

// ── Контакты (TOFU) ─────────────────────────────────────────────────────────

/// Добавить контакт. Возвращает "added" | "known" | "error".
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_addContact<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
    name: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "error"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "error") };
        // Принимаем и голый hex личности, и расширенный код обмена с подписанной
        // привязкой к адресу (ADR-007) — разбирает движок.
        let code = jstr(&mut env, &key_hex);
        let name = jstr(&mut env, &name);
        ret(&env, e.add_contact_code(&code, &name))
    })
}

/// Вывести ключ шифрования из пароля (Argon2id) — уровень at-rest «пароль».
/// Долгая операция (~0.5–1 с на телефоне) — звать с фонового потока. Пустой массив
/// при ошибке (пустой пароль/неверная соль).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_deriveVaultKey<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    password: JString<'local>,
    salt: JByteArray<'local>,
    profile: jint,
) -> JByteArray<'local> {
    let empty = empty_bytes(&env);
    jni_safe!(empty_bytes(&env), {
        let pass = jstr(&mut env, &password);
        let salt_bytes = env.convert_byte_array(&salt).unwrap_or_default();
        match peerage_core::kdf::derive_key_with(profile.max(0) as u8, &pass, &salt_bytes) {
            Ok(key) => env.byte_array_from_slice(&key).unwrap_or(empty),
            Err(e) => {
                log::warn!("derive_vault_key: {e}");
                empty
            }
        }
    })
}

/// Номер профиля стоимости KDF для НОВЫХ файлов. Живёт в ядре, а не в Kotlin:
/// иначе при повышении стоимости пришлось бы помнить про две константы.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_currentKdfProfile<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    jni_safe!(0, { peerage_core::kdf::CURRENT_PROFILE as jint })
}

/// Свежая случайная соль для KDF (16 байт).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_newVaultSalt<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JByteArray<'local> {
    let empty = empty_bytes(&env);
    jni_safe!(empty_bytes(&env), {
        env.byte_array_from_slice(&peerage_core::kdf::new_salt()).unwrap_or(empty)
    })
}

/// Что и сколько войдёт в резервную копию: JSON `{contacts, messages, notes, calls,
/// attachments, attachment_bytes, skipped_big}`. Считает по базе — с фонового потока.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_backupEstimate<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.backup_estimate_json())
    })
}

/// Создать резервную копию данных в файл `path`, зашифровав `password`.
/// `partsJson` — что положить: `{"contacts":true,"messages":true,...}`.
/// Возвращает JSON статистики или `{"error":"..."}`. Долгая операция (KDF + вложения).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_backupExport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    path: JString<'local>,
    parts_json: JString<'local>,
    password: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "{\"error\":\"внутренняя ошибка\"}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else {
            return ret(&env, "{\"error\":\"движок не запущен\"}");
        };
        let path = jstr(&mut env, &path);
        let parts = jstr(&mut env, &parts_json);
        let pass = jstr(&mut env, &password);
        ret(&env, &e.backup_export_json(&path, &parts, &pass))
    })
}

/// Восстановить из копии `path`. Возвращает JSON ДОБАВЛЕННОГО или `{"error":"..."}`.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_backupImport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    path: JString<'local>,
    password: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, "{\"error\":\"внутренняя ошибка\"}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else {
            return ret(&env, "{\"error\":\"движок не запущен\"}");
        };
        let path = jstr(&mut env, &path);
        let pass = jstr(&mut env, &password);
        ret(&env, &e.backup_import_json(&path, &pass))
    })
}

/// Кто сейчас флудит: JSON `[{peer, dropped, pps, blocked}]`, по убыванию темпа.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_floodPeers<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.flood_json())
    })
}

/// Текущая эпоха адреса (0 — адрес совпадает с ключом личности).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_transportEpoch<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        e.transport_epoch() as jint
    })
}

/// Сменить адрес: гашение узла → пауза офлайн → подъём на новом ключе → рассылка
/// подписанного объявления перечисленным контактам (hex через запятую).
/// Fire-and-forget: операция долгая (пауза!), результат придёт событием.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_rotateTransport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    recipients_csv: JString<'local>,
    gap_secs: jint,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        let csv = jstr(&mut env, &recipients_csv);
        let recipients: Vec<[u8; 32]> =
            csv.split(',').map(str::trim).filter(|s| !s.is_empty()).filter_map(parse_key).collect();
        e.rotate_transport(recipients, gap_secs.max(0) as u64);
    });
}

/// Уйти в офлайн (разорвать соединения) — перестать платить за чужой поток.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_goOffline<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.go_offline(); }
    });
}

/// Вернуться в сеть на текущем адресе.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_goOnline<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        if e.go_online() { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Мы намеренно вне сети (офлайн по кнопке или пауза смены адреса).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_isOffline<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jboolean {
    jni_safe!(JNI_FALSE, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return JNI_FALSE };
        if e.is_offline() { JNI_TRUE } else { JNI_FALSE }
    })
}

/// Строка для своего QR/«скопировать ключ»: личность, а после смены адреса — ещё и
/// подписанная привязка к текущему адресу.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_myContactCode<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        ret(&env, &e.my_contact_code())
    })
}

/// Снимок офлайн-очереди: JSON {pending, bytes, peers, warn}. По `warn` клиент
/// показывает предупреждение о разросшейся очереди (ничего не удаляется само).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_outboxStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.outbox_stats_json())
    })
}

/// Заметки себе («Сохранённое») — JSON-история чата с самим собой.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_notes<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.notes_json())
    })
}

/// Сохранить заметку себе. Возвращает id (или -1).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_saveNote<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    text: JString<'local>,
) -> jlong {
    jni_safe!(-1, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return -1 };
        let t = jstr(&mut env, &text);
        e.save_note(&t).unwrap_or(-1)
    })
}

/// Удалить заметку по id.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_deleteNote<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    id: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } {
            e.delete_note(id.max(0) as u64);
        }
    })
}

/// Журнал звонков (JSON-массив, свежие первыми).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_callLog<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    limit: jint,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.calls_json(limit.max(0) as usize))
    })
}

/// Очистить журнал звонков (явное действие пользователя).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_clearCallLog<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } {
            e.clear_calls();
        }
    })
}

/// Визуальный отпечаток ключа (hex, 12 байт) — сверка глазами по сетке знаков.
/// Пустой `key_hex` — свой отпечаток.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_visualFingerprint<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) -> jstring {
    jni_safe!(ret(&env, ""), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "") };
        let hex_str = jstr(&mut env, &key_hex);
        let out = match parse_key(&hex_str) {
            Some(key) => e.visual_fingerprint(&key),
            None => e.visual_fingerprint_self(),
        };
        ret(&env, &out)
    })
}

/// Очередь по получателям (JSON-массив) для экрана очереди отправки.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_outboxByPeer<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "[]"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "[]") };
        ret(&env, &e.outbox_by_peer_json())
    })
}

/// Сбросить очередь ВСЕМ (двойное подтверждение — на стороне UI).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_clearOutboxAll<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return 0 };
        e.clear_outbox_all() as jint
    })
}

/// Сбросить очередь недоставленного пиру (явное действие пользователя).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_clearOutboxFor<'local>(
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

/// Очистить историю личного чата у себя (сообщения, закрепы, очередь отправки).
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_clearDmHistory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
    key_hex: JString<'local>,
) {
    jni_safe!((), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return };
        if let Some(k) = parse_key(&jstr(&mut env, &key_hex)) {
            e.clear_dm_history(&k);
        }
    });
}

/// Что занимает место: JSON {blobs, blob_bytes, outbox_pending, outbox_bytes}.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_storageStats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jstring {
    jni_safe!(ret(&env, "{}"), {
        let Some(e) = (unsafe { engine(session_ptr) }) else { return ret(&env, "{}") };
        ret(&env, &e.storage_stats_json())
    })
}

/// Удалить все сохранённые вложения (явное действие). Возвращает освобождённые байты.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_clearBlobs<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jlong {
    jni_safe!(0, {
        unsafe { engine(session_ptr) }.map(|e| e.clear_blobs() as jlong).unwrap_or(0)
    })
}

/// Список контактов как JSON-массив.
#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_listContacts<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_setVerified<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_renameContact<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_removeContact<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_contactRequests<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_acceptContactRequest<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_declineContactRequest<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_blockContact<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_unblockContact<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_isOnline<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_netPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) -> jint {
    jni_safe!(0, {
        unsafe { engine(session_ptr) }.map(|e| e.net_peers()).unwrap_or(0)
    })
}

#[no_mangle]
pub extern "system" fn Java_chat_peerage_Native_netTree<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_netStats<'local>(
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
pub extern "system" fn Java_chat_peerage_Native_reconnectPeers<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    session_ptr: jlong,
) {
    jni_safe!((), {
        if let Some(e) = unsafe { engine(session_ptr) } { e.reconnect_peers(); }
    });
}

#[cfg(test)]
mod tests {
    /// Инвариант №1: каждая точка входа JNI обёрнута в `jni_safe!`. Паника, прошедшая
    /// сквозь FFI, роняет весь процесс, а забыть обёртку в новой функции легко —
    /// поэтому проверяем исходники, а не полагаемся на ревью.
    #[test]
    fn every_jni_entry_point_is_guarded() {
        let sources = [
            ("lib.rs", include_str!("lib.rs")),
            ("call_jni.rs", include_str!("call_jni.rs")),
            ("group_jni.rs", include_str!("group_jni.rs")),
            ("bench_jni.rs", include_str!("bench_jni.rs")),
        ];
        let mut unguarded = Vec::new();
        let mut total = 0;
        for (file, src) in sources {
            let starts: Vec<usize> = src.match_indices("extern \"system\" fn ").map(|(i, _)| i).collect();
            for (n, &start) in starts.iter().enumerate() {
                let end = starts.get(n + 1).copied().unwrap_or(src.len());
                // У последней функции файла «тело» тянется до конца — отрезаем тесты.
                let body = src[start..end].split("#[cfg(test)]").next().unwrap_or("");
                total += 1;
                if !body.contains("jni_safe!") {
                    let name = body["extern \"system\" fn ".len()..].split(['<', '(']).next().unwrap_or("?");
                    unguarded.push(format!("{file}: {name}"));
                }
            }
        }
        assert!(total > 100, "нашлось подозрительно мало точек входа ({total}) — сломан разбор?");
        assert!(unguarded.is_empty(), "JNI без jni_safe!: {unguarded:#?}");
    }
}
