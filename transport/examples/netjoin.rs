//! Проба входа в глобальную сеть Yggdrasil: узел подключается к публичным пирам
//! из файла и отслеживает, попал ли он в mesh.
//!
//! Запуск (WSL): cargo run -p p2p-ygg --example netjoin -- peers/public-peers.txt
//! С логами Yggdrasil: YGG_DEBUG=1 cargo run -p p2p-ygg --example netjoin -- ...

use std::time::{Duration, Instant};

use p2p_ygg::Node;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "peers/public-peers.txt".to_string());
    let content = std::fs::read_to_string(&path).expect("не прочитать файл пиров");
    let peers: Vec<String> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();

    // Случайная идентичность для пробы.
    let mut seed = [0u8; 32];
    seed[0] = 0xA1;
    let node = Node::start(&seed).expect("старт узла");
    println!("наш адрес: {}", node.address());
    println!("пробуем {} публичных пиров...\n", peers.len());

    for p in &peers {
        match node.add_peer(p) {
            Ok(()) => {}
            Err(e) => println!("  {p}: add_peer ошибка: {e}"),
        }
    }

    let start = Instant::now();
    let mut joined = false;
    while start.elapsed() < Duration::from_secs(40) {
        std::thread::sleep(Duration::from_secs(2));
        let peers_n = node.peer_count();
        let tree_n = node.tree_count();
        println!(
            "[{:>4.0}s] прямых пиров: {peers_n}, узлов в дереве: {tree_n}",
            start.elapsed().as_secs_f64()
        );
        if peers_n >= 1 && tree_n > 2 && !joined {
            joined = true;
            println!("✓ вошли в глобальную сеть Yggdrasil");
        }
    }

    if joined {
        println!("\n✓ узел в сети. Глобальный адрес: {}", node.address());
    } else {
        println!("\n✗ не удалось войти в сеть (нет связи с публичными пирами?)");
        std::process::exit(1);
    }
}
