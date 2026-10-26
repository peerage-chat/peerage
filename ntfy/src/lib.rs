//! peerage-ntfy — минимальный клиент [ntfy](https://ntfy.sh): публикация и подписка.
//!
//! Зачем: (1) push-пробуждение приложения в фоне; (2) delivery service для групп
//! (топик = группа, ntfy даёт FIFO-порядок и хранение для офлайн-участников).
//!
//! Приватность: через сервер идут только БАЙТЫ (мы кладём зашифрованное —
//! MLS-сообщения или зашифрованные wake-токены) в кодировке base64. Сервер видит
//! имя топика, размер и время — но не содержимое и не участников.

use std::io::{BufRead, BufReader};

use base64::Engine as _;

#[derive(Debug, thiserror::Error)]
pub enum NtfyError {
    #[error("http: {0}")]
    Http(String),
    #[error("base64: {0}")]
    B64(#[from] base64::DecodeError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Опубликовать байты в топик (тело base64). `server` — напр. "https://ntfy.sh".
pub fn publish(server: &str, topic: &str, data: &[u8]) -> Result<(), NtfyError> {
    let url = format!("{}/{}", server.trim_end_matches('/'), topic);
    let body = b64().encode(data);
    ureq::post(&url)
        .send_string(&body)
        .map_err(|e| NtfyError::Http(e.to_string()))?;
    Ok(())
}

/// Подписаться на топик и вызывать `on_message` для каждого пришедшего сообщения
/// (декодированные байты). Блокирующе: держит стрим открытым, пока соединение
/// живо. Запускайте в отдельном потоке.
///
/// `since_all` = true — получить и сохранённую историю (полезно для догоняющего
/// участника группы), иначе только новые сообщения.
pub fn subscribe<F: FnMut(Vec<u8>)>(
    server: &str,
    topic: &str,
    since_all: bool,
    mut on_message: F,
) -> Result<(), NtfyError> {
    let mut url = format!("{}/{}/json", server.trim_end_matches('/'), topic);
    if since_all {
        url.push_str("?since=all");
    }
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| NtfyError::Http(e.to_string()))?;
    let reader = BufReader::new(resp.into_reader());
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v["event"] == "message" {
            if let Some(m) = v["message"].as_str() {
                if let Ok(bytes) = b64().decode(m) {
                    on_message(bytes);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    #[ignore = "сетевой тест: требует доступ к ntfy.sh"]
    fn publish_subscribe_roundtrip() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let topic = format!("test-selftest-{nanos}");
        let server = "https://ntfy.sh";
        let payload = b"\x00\x01\x02 binary group bytes \xfe\xff".to_vec();

        let (tx, rx) = mpsc::channel();
        let t = topic.clone();
        let p = payload.clone();
        std::thread::spawn(move || {
            let _ = subscribe(server, &t, false, |bytes| {
                if bytes == p {
                    let _ = tx.send(());
                }
            });
        });

        std::thread::sleep(Duration::from_secs(2)); // дать подписке подключиться
        publish(server, &topic, &payload).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(15)).is_ok(), "сообщение не дошло");
    }
}
