//! Узлы входа: что настроил пользователь, то и получает транспорт — сразу и после
//! перезапуска.
//!
//! Прогон из WSL: LD_LIBRARY_PATH=ygg/libygg.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use peerage_core::Identity;
use peerage_engine::peers::{self, AddError};
use peerage_engine::Engine;
use peerage_messenger::loopback::{LoopbackNet, LoopbackTransport};
use peerage_messenger::{PeerKey, Transport};
use peerage_ygg::YggError;

/// Loopback-транспорт, который записывает, к кому ему велели подключаться.
struct Recording {
    inner: Arc<LoopbackTransport>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Transport for Recording {
    fn write(&self, dst: &PeerKey, data: &[u8]) -> Result<usize, YggError> {
        self.inner.write(dst, data)
    }
    fn read(&self, buf: &mut [u8]) -> Result<(usize, PeerKey), YggError> {
        self.inner.read(buf)
    }
    fn address(&self) -> String {
        self.inner.address()
    }
    fn peer_count(&self) -> i32 {
        self.inner.peer_count()
    }
    fn tree_count(&self) -> i32 {
        self.inner.tree_count()
    }
    fn peers_json(&self) -> String {
        self.inner.peers_json()
    }
    fn add_peer(&self, uri: &str) -> Result<(), YggError> {
        self.log.lock().unwrap().push(format!("+{uri}"));
        Ok(())
    }
    fn remove_peer(&self, uri: &str) -> Result<(), YggError> {
        self.log.lock().unwrap().push(format!("-{uri}"));
        Ok(())
    }
    fn stop(&self) {
        self.inner.stop()
    }
}

fn tmpdir() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("test-entry-nodes-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(net: &LoopbackNet, dir: &PathBuf, log: &Arc<Mutex<Vec<String>>>) -> Arc<Engine> {
    let ident = Identity::from_seed(&[7; 32]).unwrap();
    let t = Recording {
        inner: net.endpoint(ident.public_key_bytes()),
        log: log.clone(),
    };
    Engine::start_with_transport(dir.clone(), ident, Arc::new(t))
        .unwrap()
        .engine
}

fn take(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

#[test]
fn own_nodes_and_builtin_switch_reach_the_transport_and_survive_restart() {
    let net = LoopbackNet::new();
    let dir = tmpdir();
    let log = Arc::new(Mutex::new(Vec::new()));
    let builtin: Vec<String> = peers::builtin().map(str::to_owned).collect();

    let e = start(&net, &dir, &log);
    let s = e.peer_settings();
    assert!(s.builtin_on && s.own.is_empty());
    assert_eq!(s.builtin, builtin);
    take(&log);

    // Свой узел: подключение сразу, без перезапуска.
    assert_eq!(e.peers_add(" tls://ygg.home.example:443 "), Ok(()));
    assert_eq!(take(&log), ["+tls://ygg.home.example:443"]);
    assert_eq!(
        e.peers_add("tls://ygg.home.example:443"),
        Err(AddError::Duplicate)
    );
    assert_eq!(e.peers_add("ygg.home.example"), Err(AddError::Invalid));
    assert!(take(&log).is_empty(), "отказ не трогает транспорт");

    // Выключить узлы из сборки: каждый снят.
    e.peers_set_builtin(false);
    let expect: Vec<String> = builtin.iter().map(|u| format!("-{u}")).collect();
    assert_eq!(take(&log), expect);
    e.peers_set_builtin(false);
    assert!(take(&log).is_empty(), "повтор ничего не меняет");

    // Переподключение — только к тому, что включено.
    e.reconnect_peers();
    assert_eq!(take(&log), ["+tls://ygg.home.example:443"]);

    e.peers_add("quic://[2001:db8::1]:9001").unwrap();
    e.peers_remove("tls://ygg.home.example:443");
    assert_eq!(
        take(&log),
        ["+quic://[2001:db8::1]:9001", "-tls://ygg.home.example:443"]
    );
    e.peers_remove("tls://ygg.home.example:443");
    assert!(take(&log).is_empty(), "убрать отсутствующий — no-op");
    e.shutdown();
    drop(e);

    // Перезапуск: настройка та же.
    let e = start(&net, &dir, &log);
    let s = e.peer_settings();
    assert!(!s.builtin_on);
    assert_eq!(s.own, ["quic://[2001:db8::1]:9001"]);
    take(&log);
    e.peers_set_builtin(true);
    let expect: Vec<String> = builtin.iter().map(|u| format!("+{u}")).collect();
    assert_eq!(take(&log), expect);
    e.shutdown();
    std::fs::remove_dir_all(&dir).ok();
}
