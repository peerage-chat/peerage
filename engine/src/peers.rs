//! Узлы входа Yggdrasil: к кому узел подключается сам.
//!
//! Два источника: список из сборки (`peers/public-peers.txt`, обновляется с
//! приложением, включается одним тумблером) и свои адреса пользователя. Настройка
//! лежит в каталоге профиля (`peers.json`); файла нет — всё по умолчанию: узлы из
//! сборки включены, своих нет. Это не протокол: другой стороне настройка не видна.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BUILTIN: &str = include_str!("../../peers/public-peers.txt");

/// Узлы из сборки: строки файла без пустых и комментариев.
pub fn builtin() -> impl Iterator<Item = &'static str> {
    BUILTIN
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Почему адрес не добавлен.
#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    /// Не `tls://`, `tcp://` или `quic://` с хостом и портом.
    Invalid,
    /// Такой адрес уже есть — свой или из сборки.
    Duplicate,
}

impl AddError {
    /// Код для клиента (строка ответа JNI).
    pub fn code(&self) -> &'static str {
        match self {
            AddError::Invalid => "invalid",
            AddError::Duplicate => "duplicate",
        }
    }
}

/// Привести адрес узла к виду, который примет Yggdrasil, или отказать.
/// Схема — tls/tcp/quic, хост (имя, IPv4 или IPv6 в скобках), порт 1–65535; после
/// порта допустим запрос (`?key=…`, `?sni=…`). Схема приводится к нижнему регистру.
pub fn parse_uri(s: &str) -> Result<String, AddError> {
    let s = s.trim();
    let (scheme, rest) = s.split_once("://").ok_or(AddError::Invalid)?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "tls" | "tcp" | "quic") {
        return Err(AddError::Invalid);
    }
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, p) = v6.split_once(']').ok_or(AddError::Invalid)?;
        (h, p.strip_prefix(':').ok_or(AddError::Invalid)?)
    } else {
        authority.rsplit_once(':').ok_or(AddError::Invalid)?
    };
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'));
    let port_ok = !port.is_empty()
        && port.chars().all(|c| c.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|p| p > 0);
    if !host_ok || !port_ok || tail.chars().any(char::is_whitespace) {
        return Err(AddError::Invalid);
    }
    Ok(format!("{scheme}://{rest}"))
}

/// Настройка узлов входа (`peers.json` в каталоге профиля).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PeerConfig {
    /// Подключаться к узлам из сборки.
    pub builtin: bool,
    /// Свои адреса в порядке добавления.
    pub own: Vec<String>,
}

impl Default for PeerConfig {
    fn default() -> Self {
        PeerConfig {
            builtin: true,
            own: Vec::new(),
        }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("peers.json")
}

impl PeerConfig {
    /// Прочитать настройку; файла нет или он не читается — по умолчанию.
    pub fn load(dir: &Path) -> Self {
        std::fs::read(path(dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let v = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        peerage_core::fsutil::write_atomic(&path(dir), &v)
    }

    /// Все адреса, к которым узел подключается: из сборки (если включены) и свои.
    pub fn uris(&self) -> Vec<String> {
        let b = builtin().filter(|_| self.builtin).map(str::to_owned);
        b.chain(self.own.iter().cloned()).collect()
    }

    /// Добавить свой адрес. Возвращает его в приведённом виде.
    pub fn add(&mut self, uri: &str) -> Result<String, AddError> {
        let uri = parse_uri(uri)?;
        if self.own.contains(&uri) || builtin().any(|b| b == uri) {
            return Err(AddError::Duplicate);
        }
        self.own.push(uri.clone());
        Ok(uri)
    }

    /// Убрать свой адрес. `false` — такого не было.
    pub fn remove(&mut self, uri: &str) -> bool {
        let before = self.own.len();
        self.own.retain(|u| u != uri);
        self.own.len() != before
    }
}

/// Снимок для экрана «Узлы входа».
#[derive(Serialize, Debug, PartialEq)]
pub struct PeerSettings {
    pub builtin_on: bool,
    pub builtin: Vec<String>,
    pub own: Vec<String>,
}

impl From<&PeerConfig> for PeerSettings {
    fn from(c: &PeerConfig) -> Self {
        PeerSettings {
            builtin_on: c.builtin,
            builtin: builtin().map(str::to_owned).collect(),
            own: c.own.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_three_schemes_with_port() {
        for ok in [
            "tls://ygg.home.example:443",
            "tcp://203.0.113.7:9001",
            "quic://[2001:db8::1]:9001",
            "  TLS://ygg.example:1338?key=abcd  ",
            "tls://ygg.example:443/?sni=ygg.example",
        ] {
            assert!(parse_uri(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            parse_uri("TLS://ygg.example:1338").unwrap(),
            "tls://ygg.example:1338"
        );
    }

    #[test]
    fn rejects_what_yggdrasil_would_not_dial() {
        for bad in [
            "",
            "ygg.home.example",
            "ygg.home.example:443",
            "http://ygg.example:80",
            "socks://127.0.0.1:9050/ygg.example:443",
            "tls://ygg.example",
            "tls://ygg.example:",
            "tls://ygg.example:0",
            "tls://ygg.example:70000",
            "tls://ygg.example:44x",
            "tls://:443",
            "tls://[2001:db8::1]9001",
            "tls://ygg ex.ample:443",
            "tls://ygg.example:443?key=a b",
        ] {
            assert_eq!(parse_uri(bad), Err(AddError::Invalid), "{bad}");
        }
    }

    #[test]
    fn duplicates_are_refused_own_and_builtin() {
        let mut c = PeerConfig::default();
        assert_eq!(
            c.add("tls://ygg.home.example:443").as_deref(),
            Ok("tls://ygg.home.example:443")
        );
        assert_eq!(
            c.add("TLS://ygg.home.example:443"),
            Err(AddError::Duplicate)
        );
        let first = builtin().next().unwrap();
        assert_eq!(c.add(first), Err(AddError::Duplicate));
    }

    #[test]
    fn uris_follow_the_builtin_switch_and_own_order() {
        let mut c = PeerConfig::default();
        c.add("tcp://203.0.113.7:9001").unwrap();
        c.add("tls://ygg.home.example:443").unwrap();
        let n = builtin().count();
        assert!(n > 0);
        assert_eq!(c.uris().len(), n + 2);
        c.builtin = false;
        assert_eq!(
            c.uris(),
            ["tcp://203.0.113.7:9001", "tls://ygg.home.example:443"]
        );
        assert!(c.remove("tcp://203.0.113.7:9001"));
        assert!(!c.remove("tcp://203.0.113.7:9001"));
        assert_eq!(c.uris(), ["tls://ygg.home.example:443"]);
    }

    #[test]
    fn config_survives_restart_and_missing_file_is_default() {
        let dir = std::env::temp_dir().join(format!("peerage-peers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(PeerConfig::load(&dir), PeerConfig::default());
        let mut c = PeerConfig {
            builtin: false,
            own: Vec::new(),
        };
        c.add("quic://[2001:db8::1]:9001").unwrap();
        c.save(&dir).unwrap();
        assert_eq!(PeerConfig::load(&dir), c);
        std::fs::remove_dir_all(&dir).ok();
    }
}
