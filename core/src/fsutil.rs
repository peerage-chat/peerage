//! Атомарная запись файлов состояния.
//!
//! Голый `fs::write` сначала обрезает файл до нуля и лишь потом пишет. Android
//! убивает процесс в любой момент, и если это попало между двумя шагами, на диске
//! остаётся пустой или обрезанный файл: для MLS-состояния это сломанная группа, для
//! профиля — потерянная личность. Поэтому пишем во временный файл рядом, сбрасываем
//! его на диск и переименовываем поверх старого: читатель видит либо прежнюю версию,
//! либо новую целиком, но не промежуточную.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Записать файл атомарно (права по умолчанию). Сигнатура как у `fs::write`.
pub fn write_atomic(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> io::Result<()> {
    write_via_temp(path.as_ref(), data.as_ref(), false)
}

/// Записать файл атомарно, доступный только владельцу (0600 на unix): ключи и сиды.
pub fn write_atomic_private(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> io::Result<()> {
    write_via_temp(path.as_ref(), data.as_ref(), true)
}

fn write_via_temp(path: &Path, data: &[u8], private: bool) -> io::Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let mut f = create_new(&tmp, private)?;
        f.write_all(data)?;
        // Без sync rename может долететь до диска раньше данных: при отключении
        // питания получили бы «новый» файл нулевой длины.
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        // Сам rename — запись в каталог: без его sync на части ФС переименование
        // может не пережить отключение питания, и вернётся старая версия.
        if let Some(dir) = path.parent() {
            sync_dir(dir);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn sync_dir(dir: &Path) {
    // Ошибку глотаем: не каждая ФС даёт sync каталога, а данные уже на месте.
    if let Ok(d) = File::open(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) {
        let _ = d.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

/// Удалить обрывки временных файлов в каталоге (не рекурсивно). Процесс, убитый
/// посреди [`write_atomic`], оставляет `*.tmp`: подобрать их некому, а в каталоге
/// блобов они ещё и засчитываются в занятое место. Звать на старте, пока никто
/// не пишет. Возвращает, сколько удалено.
pub fn sweep_temp(dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .filter(|e| fs::remove_file(e.path()).is_ok())
        .count()
}

/// Имя временного файла рядом с целевым (тот же каталог — значит, та же ФС и
/// rename атомарен). Уникально внутри процесса: один файл могут писать два потока.
fn temp_path(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.{}.{n}.tmp", std::process::id()))
}

#[cfg(unix)]
fn create_new(path: &Path, private: bool) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    if private {
        o.mode(0o600);
    }
    o.open(path)
}

#[cfg(not(unix))]
fn create_new(path: &Path, _private: bool) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("p2p-fsutil-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn overwrites_and_leaves_no_temp_files() {
        let d = dir("overwrite");
        let p = d.join("state.bin");
        write_atomic(&p, b"first").unwrap();
        write_atomic(&p, b"second, longer").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"second, longer");
        let names: Vec<_> = fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("state.bin")], "временный файл остался");
    }

    #[test]
    fn failed_write_keeps_the_old_version() {
        let d = dir("fail");
        let p = d.join("state.bin");
        write_atomic(&p, b"old").unwrap();
        // Каталог на месте целевого файла: rename обязан упасть, а старые данные — остаться.
        let target_dir = d.join("sub");
        fs::create_dir_all(target_dir.join("x")).unwrap();
        assert!(write_atomic(&target_dir, b"new").is_err());
        assert_eq!(fs::read(&p).unwrap(), b"old");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2, "за сбоем не прибрали временный файл");
    }

    #[test]
    fn sweep_removes_only_temp_leftovers() {
        let d = dir("sweep");
        fs::write(d.join("state.bin.123.0.tmp"), b"x").unwrap();
        fs::write(d.join("state.bin"), b"keep").unwrap();
        fs::write(d.join("blob.part"), b"keep").unwrap();
        assert_eq!(sweep_temp(&d), 1);
        let mut names: Vec<_> = fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        names.sort();
        assert_eq!(names, ["blob.part", "state.bin"]);
    }

    #[cfg(unix)]
    #[test]
    fn private_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let p = dir("private").join("key");
        write_atomic_private(&p, &[7u8; 32]).unwrap();
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
