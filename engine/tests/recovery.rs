//! R-029: не подошёл ключ хранилища — данные не стираются молча. Движок сообщает
//! об этом клиенту, а по выбору человека данные откладываются в карантин.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use p2p_engine::{load_identity, quarantine_user_data, QUARANTINE_DIR, START_VAULT_MISMATCH};

fn tmpdir(tag: &str) -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("p2p-recovery-{tag}-{n}"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Профиль, запечатанный ключом, которого у нас нет: маркер «зашифровано» и мусор.
fn sealed_by_someone_else() -> Vec<u8> {
    let mut v = vec![1u8];
    v.extend((0..80u8).map(|i| i.wrapping_mul(37)));
    v
}

#[test]
fn foreign_key_is_reported_not_wiped() {
    let dir = tmpdir("mismatch");
    let profile = sealed_by_someone_else();
    std::fs::write(dir.join("profile.json"), &profile).unwrap();
    std::fs::write(dir.join("messenger.db"), b"old history").unwrap();

    let err = load_identity(&dir).err().expect("чужой ключ — ошибка, а не новая личность");
    assert_eq!(err, START_VAULT_MISMATCH);
    assert_eq!(std::fs::read(dir.join("profile.json")).unwrap(), profile, "профиль не тронут");
    assert!(dir.join("messenger.db").exists(), "история не стёрта");
}

#[test]
fn quarantine_sets_data_aside_and_keeps_the_key_wrapper() {
    let dir = tmpdir("quarantine");
    let profile = sealed_by_someone_else();
    std::fs::write(dir.join("profile.json"), &profile).unwrap();
    std::fs::write(dir.join("messenger.db"), b"old history").unwrap();
    std::fs::create_dir_all(dir.join("groups")).unwrap();
    std::fs::write(dir.join("groups").join("g.json"), b"group").unwrap();
    std::fs::create_dir_all(dir.join("blobs")).unwrap();
    std::fs::write(dir.join("vault.key.enc"), b"wrapped key").unwrap();
    std::fs::write(dir.join("vault.meta"), b"KEYSTORE").unwrap();

    let dst = quarantine_user_data(&dir).unwrap();
    assert!(dst.starts_with(dir.join(QUARANTINE_DIR)));
    // Отложено целиком и байт-в-байт.
    assert_eq!(std::fs::read(dst.join("profile.json")).unwrap(), profile);
    assert_eq!(std::fs::read(dst.join("messenger.db")).unwrap(), b"old history");
    assert_eq!(std::fs::read(dst.join("groups").join("g.json")).unwrap(), b"group");
    assert!(dst.join("blobs").is_dir());
    // На месте — только обёртка ключа и сам карантин.
    let mut left: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, vec![QUARANTINE_DIR.to_string(), "vault.key.enc".into(), "vault.meta".into()]);

    // Чистый каталог — новая личность заводится как при первом запуске.
    assert!(load_identity(&dir).is_ok());
    assert!(dir.join("profile.json").exists());

    // Повторный карантин не трогает прежний и не теряет его.
    let again = quarantine_user_data(&dir).unwrap();
    assert_ne!(again, dst);
    assert!(dst.join("profile.json").exists(), "прежний карантин цел");
    assert!(again.join("profile.json").exists());
}
