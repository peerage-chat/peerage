//! Типизированные снапшоты сети и групп + ЧИСТЫЕ агрегаты (без устройства/транспорта).
//! Эти функции — ядро контракта ADR-003: домейн-логику (агрегаты) считает движок,
//! клиент лишь рендерит. Тестируются юнитами без эмулятора/libygg.

use serde::{Deserialize, Serialize};

/// Пир транспорта (адрес/направление/пинг/трафик). Десериализуется из libygg JSON.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PeerStat {
    pub key: String,
    /// Адрес, по которому идёт соединение: у исходящих — URI узла входа (по нему
    /// экран «Узлы входа» ставит статус), у входящих — откуда пришли.
    #[serde(default)]
    pub uri: String,
    pub inbound: bool,
    pub up: bool,
    pub latency_ms: f64,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub uptime_s: f64,
    /// Адрес меша, выведенный из ключа (в JSON libygg его нет — считает
    /// [`NetStats::aggregate`]). Пусто — ключа ещё нет (соединение не поднялось).
    #[serde(default)]
    pub addr: String,
}

/// Адрес Yggdrasil по hex-ключу пира; пусто, если ключ не 32 байта.
fn addr_of(key_hex: &str) -> String {
    let mut key = [0u8; 32];
    match hex::decode_to_slice(key_hex, &mut key) {
        Ok(()) => peerage_core::identity::addr_for_key(&key).to_string(),
        Err(_) => String::new(),
    }
}

/// Снапшот стата сети: пиры + агрегаты (число/пинг/трафик).
#[derive(Serialize, Debug, PartialEq)]
pub struct NetStats {
    pub peer_count: usize,
    pub tree: i32,
    /// Лучший (минимальный) пинг среди up-пиров; 0 если неизвестен.
    pub ping_ms: f64,
    /// Суммарный входящий/исходящий байт/с.
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub peers: Vec<PeerStat>,
}

impl NetStats {
    /// Свернуть список пиров в агрегаты. Чистая функция — тестируется без сети.
    pub fn aggregate(mut peers: Vec<PeerStat>, tree: i32) -> NetStats {
        for p in &mut peers {
            p.addr = addr_of(&p.key);
        }
        let best_ping = peers
            .iter()
            .filter(|p| p.up && p.latency_ms > 0.0)
            .map(|p| p.latency_ms)
            .fold(f64::INFINITY, f64::min);
        NetStats {
            peer_count: peers.len(),
            tree,
            ping_ms: if best_ping.is_finite() {
                best_ping
            } else {
                0.0
            },
            rx_rate: peers.iter().map(|p| p.rx_rate).sum(),
            tx_rate: peers.iter().map(|p| p.tx_rate).sum(),
            peers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(up: bool, lat: f64, rx: u64, tx: u64) -> PeerStat {
        PeerStat {
            key: "ab".into(),
            uri: String::new(),
            inbound: false,
            up,
            latency_ms: lat,
            rx_rate: rx,
            tx_rate: tx,
            uptime_s: 1.0,
            addr: String::new(),
        }
    }

    /// Адрес пира выводит движок: клиент не знает алгоритма Yggdrasil.
    #[test]
    fn netstats_derives_peer_address() {
        let key = [7u8; 32];
        let mut p = peer(true, 10.0, 0, 0);
        p.key = hex::encode(key);
        let mut pending = peer(false, 0.0, 0, 0);
        pending.key = String::new();
        let s = NetStats::aggregate(vec![p, pending], 1);
        let want = peerage_core::identity::addr_for_key(&key).to_string();
        assert!(want.starts_with("2"), "адрес меша: {want}");
        assert_eq!(s.peers[0].addr, want);
        assert_eq!(s.peers[1].addr, "", "без ключа адреса нет");
    }

    #[test]
    fn netstats_empty() {
        let s = NetStats::aggregate(vec![], 0);
        assert_eq!(s.peer_count, 0);
        assert_eq!(s.ping_ms, 0.0);
        assert_eq!(s.rx_rate, 0);
    }

    #[test]
    fn netstats_best_ping_among_up_peers() {
        let s = NetStats::aggregate(
            vec![
                peer(true, 120.0, 10, 1),
                peer(true, 64.0, 20, 2),
                peer(false, 5.0, 99, 99),
            ],
            3,
        );
        assert_eq!(s.peer_count, 3);
        assert_eq!(s.tree, 3);
        // down-пир с пингом 5 не учитывается; лучший среди up — 64.
        assert_eq!(s.ping_ms, 64.0);
        // суммы по ВСЕМ пирам.
        assert_eq!(s.rx_rate, 10 + 20 + 99);
        assert_eq!(s.tx_rate, 1 + 2 + 99);
    }

    #[test]
    fn netstats_ignores_zero_latency() {
        // up-пир с latency 0 (ещё не измерен) не должен давать ping_ms=0 ложно.
        let s = NetStats::aggregate(vec![peer(true, 0.0, 1, 1), peer(true, 80.0, 1, 1)], 1);
        assert_eq!(s.ping_ms, 80.0);
    }
}
