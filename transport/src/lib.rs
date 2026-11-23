//! peerage-ygg — безопасная Rust-обёртка над libygg (ядро yggdrasil-go через FFI).
//!
//! Предоставляет датаграммный транспорт поверх mesh-сети Yggdrasil,
//! адресуемый по 32-байтному публичному ключу ed25519 узла. Без TUN, без root.
//!
//! ```no_run
//! use peerage_ygg::Node;
//! let node = Node::start(&[0u8; 32]).unwrap();
//! println!("адрес: {}", node.address());
//! ```

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

mod ffi {
    use std::os::raw::{c_char, c_int};
    extern "C" {
        pub fn ygg_start(seed_ptr: *const u8, seed_len: c_int) -> u64;
        pub fn ygg_address(h: u64, out: *mut c_char, out_cap: c_int) -> c_int;
        pub fn ygg_public_key(h: u64, out: *mut u8) -> c_int;
        pub fn ygg_listen(
            h: u64,
            uri: *const c_char,
            out: *mut c_char,
            out_cap: c_int,
        ) -> c_int;
        pub fn ygg_peer_count(h: u64) -> c_int;
        pub fn ygg_peers_json(h: u64, out: *mut c_char, out_cap: c_int) -> c_int;
        pub fn ygg_tree_count(h: u64) -> c_int;
        pub fn ygg_add_peer(h: u64, uri: *const c_char) -> c_int;
        pub fn ygg_remove_peer(h: u64, uri: *const c_char) -> c_int;
        pub fn ygg_write(h: u64, dst_pub: *const u8, data: *const u8, data_len: c_int) -> c_int;
        pub fn ygg_write_media(
            h: u64,
            dst_pub: *const u8,
            data: *const u8,
            data_len: c_int,
            urgent: c_int,
        ) -> c_int;
        pub fn ygg_read(h: u64, out: *mut u8, out_cap: c_int, src_pub: *mut u8) -> c_int;
        pub fn ygg_block(h: u64, key: *const u8) -> c_int;
        pub fn ygg_unblock(h: u64, key: *const u8) -> c_int;
        pub fn ygg_dropped(h: u64) -> i64;
        pub fn ygg_stop(h: u64);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum YggError {
    #[error("не удалось запустить узел Yggdrasil (неверный seed?)")]
    StartFailed,
    #[error("операция Yggdrasil завершилась ошибкой")]
    OpFailed,
    #[error("в строке URI есть нулевой байт")]
    InvalidUri(#[from] std::ffi::NulError),
}

/// Узел Yggdrasil. Содержит только handle; потокобезопасен (FFI-сторона
/// синхронизирована). Оборачивайте в `Arc` для использования из нескольких
/// потоков (например, блокирующий [`Node::read`] в отдельном потоке).
pub struct Node {
    handle: u64,
}

// Node содержит только u64-handle, а libygg синхронизирует доступ к реестру
// узлов и ядро yggdrasil потокобезопасно.
unsafe impl Send for Node {}
unsafe impl Sync for Node {}

impl Node {
    /// Запустить узел из 32-байтного seed ed25519.
    pub fn start(seed: &[u8; 32]) -> Result<Node, YggError> {
        let handle = unsafe { ffi::ygg_start(seed.as_ptr(), 32) };
        if handle == 0 {
            return Err(YggError::StartFailed);
        }
        Ok(Node { handle })
    }

    /// IPv6-адрес Yggdrasil узла (строкой).
    pub fn address(&self) -> String {
        let mut buf = [0u8; 64];
        let n = unsafe {
            ffi::ygg_address(self.handle, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int)
        };
        if n < 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    }

    /// 32-байтный публичный ключ ed25519 узла.
    pub fn public_key(&self) -> [u8; 32] {
        let mut pk = [0u8; 32];
        unsafe {
            ffi::ygg_public_key(self.handle, pk.as_mut_ptr());
        }
        pk
    }

    /// Поднять слушатель по URI (напр. `"tls://127.0.0.1:0"`).
    /// Возвращает фактический URI с разрешённым портом.
    pub fn listen(&self, uri: &str) -> Result<String, YggError> {
        let c = CString::new(uri)?;
        let mut buf = [0u8; 128];
        let n = unsafe {
            ffi::ygg_listen(
                self.handle,
                c.as_ptr(),
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as c_int,
            )
        };
        if n < 0 {
            return Err(YggError::OpFailed);
        }
        Ok(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }

    /// Число активных пиров (прямых соединений).
    pub fn peer_count(&self) -> i32 {
        unsafe { ffi::ygg_peer_count(self.handle) }
    }

    /// Число узлов в дереве маршрутизации (>1 — сеть согласована).
    pub fn tree_count(&self) -> i32 {
        unsafe { ffi::ygg_tree_count(self.handle) }
    }

    /// JSON-массив активных пиров (адрес/направление/пинг/трафик) для стата сети.
    /// Пустой массив при ошибке. Буфер с запасом (~100 пиров).
    pub fn peers_json(&self) -> String {
        let mut buf = vec![0u8; 16384];
        let n = unsafe {
            ffi::ygg_peers_json(self.handle, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int)
        };
        if n < 0 {
            return "[]".into();
        }
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    }

    /// Добавить пир по URI (напр. `"tls://host:port"`).
    pub fn add_peer(&self, uri: &str) -> Result<(), YggError> {
        let c = CString::new(uri)?;
        let r = unsafe { ffi::ygg_add_peer(self.handle, c.as_ptr()) };
        if r < 0 {
            return Err(YggError::OpFailed);
        }
        Ok(())
    }

    /// Перестать подключаться к пиру `uri`, добавленному [`Node::add_peer`].
    /// Соединение рвётся не сразу.
    pub fn remove_peer(&self, uri: &str) -> Result<(), YggError> {
        let c = CString::new(uri)?;
        let r = unsafe { ffi::ygg_remove_peer(self.handle, c.as_ptr()) };
        if r < 0 {
            return Err(YggError::OpFailed);
        }
        Ok(())
    }

    /// Отправить датаграмму узлу с публичным ключом `dst_pub`.
    /// Возвращает число записанных байт. Доставка — best-effort (UDP-подобная).
    pub fn write(&self, dst_pub: &[u8; 32], data: &[u8]) -> Result<usize, YggError> {
        let n = unsafe {
            ffi::ygg_write(self.handle, dst_pub.as_ptr(), data.as_ptr(), data.len() as c_int)
        };
        if n < 0 {
            return Err(YggError::OpFailed);
        }
        Ok(n as usize)
    }

    /// Отправить медиа-датаграмму звонка. Никогда не ждёт: очередь записи полна —
    /// пакет выброшен (`Ok(false)`); опоздавшее медиа бесполезно, а ожидание здесь
    /// остановило бы потоки звонка. `urgent` — звук и служебные пакеты: их очередь
    /// пишется раньше видео.
    pub fn write_media(&self, dst_pub: &[u8; 32], data: &[u8], urgent: bool) -> Result<bool, YggError> {
        let n = unsafe {
            ffi::ygg_write_media(
                self.handle,
                dst_pub.as_ptr(),
                data.as_ptr(),
                data.len() as c_int,
                urgent as c_int,
            )
        };
        if n < 0 {
            return Err(YggError::OpFailed);
        }
        Ok(n > 0)
    }

    /// Отбрасывать пакеты от этого ключа **внутри libygg**, не поднимая их в Rust.
    ///
    /// Приёмный бюджет (`peerage_core::budget::Tier`) режет заблокированных и так, но
    /// каждый пакет до него стоит копирования через FFI и разбора заголовка — под
    /// флудом это заметная работа впустую. Байты с провода это НЕ убирает
    /// (threat-model N8), убирает только их обработку. Идемпотентно.
    pub fn block(&self, key: &[u8; 32]) {
        unsafe { ffi::ygg_block(self.handle, key.as_ptr()) };
    }

    /// Снять блокировку источника. Идемпотентно.
    pub fn unblock(&self, key: &[u8; 32]) {
        unsafe { ffi::ygg_unblock(self.handle, key.as_ptr()) };
    }

    /// Сколько пакетов отброшено чёрным списком за время жизни узла (диагностика,
    /// основа для экрана «вас флудят»).
    pub fn dropped(&self) -> u64 {
        let n = unsafe { ffi::ygg_dropped(self.handle) };
        if n < 0 {
            0
        } else {
            n as u64
        }
    }

    /// Остановить узел Yggdrasil. После этого блокирующий [`read`](Self::read)
    /// разблокируется ошибкой. Идемпотентно ([`Drop`] тоже вызывает stop).
    pub fn stop(&self) {
        unsafe { ffi::ygg_stop(self.handle) };
    }

    /// Блокирующе прочитать одну датаграмму в `buf`.
    /// Возвращает `(число_байт, публичный_ключ_отправителя)`.
    pub fn read(&self, buf: &mut [u8]) -> Result<(usize, [u8; 32]), YggError> {
        let mut src = [0u8; 32];
        let n = unsafe {
            ffi::ygg_read(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as c_int,
                src.as_mut_ptr(),
            )
        };
        if n < 0 {
            return Err(YggError::OpFailed);
        }
        Ok((n as usize, src))
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        unsafe { ffi::ygg_stop(self.handle) };
    }
}
