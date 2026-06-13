# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полное ТЗ — [p2p-messenger-spec.md](p2p-messenger-spec.md).

## Архитектура

Общее **ядро на Rust** (`core/`) — единый источник истины для протокола, крипто
и сети. Используется обеими платформами:

- **Desktop** (Windows/Linux/Mac) — Tauri, зовёт ядро напрямую.
- **Android** — Kotlin + NDK, зовёт ядро через JNI.

Транспорт — оверлейная mesh-сеть **Yggdrasil** (даёт каждому узлу постоянный
IPv6). Yggdrasil поднимает TUN-интерфейс в ОС, а ядро работает поверх через
обычные TCP/UDP сокеты на Yggdrasil-адресе.

### Структура

```
core/                 общее Rust-ядро
  src/identity.rs     идентичность: ed25519 → Yggdrasil IPv6, Curve25519, BIP39
  examples/whoami.rs  демо: печатает новую идентичность
  examples/addr.rs    адрес Yggdrasil для заданного hex-pubkey (для сверки)
transport/            Rust: безопасная обёртка над libygg (FFI к yggdrasil-go)
  src/lib.rs          Node: start/listen/add_peer/write/read по ed25519-ключу
  build.rs            линковка libygg.so + rpath
  examples/loopback.rs  два узла в процессе, обмен через настоящий Yggdrasil
ygg/                  Go: c-shared обёртка вокруг yggdrasil-go
  libygg/ygg.go       экспорт датаграммного API (ygg_start/write/read/...)
  cmd/yggaddr/        валидатор адреса средствами самого yggdrasil-go
scripts/
  build-ygg.sh          сборка libygg.so (WSL)
  run-loopback.sh       сборка + запуск loopback-теста (WSL)
  validate-ygg-addr.sh  кросс-проверка Go vs Rust вывода адреса
```

## Решения, отклоняющиеся от ТЗ

- **ed25519 как корневой ключ.** Yggdrasil (v0.4+) использует ed25519, а не
  Curve25519. Поэтому корень идентичности — ed25519, из него выводится адрес
  Yggdrasil, и он же конвертируется в Curve25519 для NaCl Box (как
  `crypto_sign_ed25519_pk_to_curve25519`).
- **RustCrypto вместо libsodium.** Крейт `crypto_box` — это та же NaCl Box
  (Curve25519 + XSalsa20 + Poly1305), бинарно совместимая с PyNaCl/libsodium, но
  чистый Rust → проще кросс-компиляция под Android, не нужен C-тулчейн.
- **Yggdrasil без TUN.** Встраиваем ядро `yggdrasil-go` как c-shared библиотеку
  и используем его `PacketConn` напрямую (датаграммы по ключу узла) вместо
  поднятия TUN-интерфейса. Не нужны права root/админа и драйвер wintun — это
  настоящее библиотечное встраивание.

## Важные технические находки

- **Сессия ironwood двунаправленная.** Зашифрованная сессия Yggdrasil (ironwood
  `iwe`) устанавливается только при обмене пакетами В ОБЕ СТОРОНЫ. Если один узел
  только шлёт, а второй только принимает — рукопожатие не завершается и пакеты
  теряются (даже при ретраях). Поэтому при установлении связи оба пира должны
  слать: на уровне протокола сообщений нужен handshake/ping с обеих сторон.

## Прогресс

- [x] Workspace + core-крейт
- [x] Идентичность: keygen ed25519, вывод адреса Yggdrasil, конвертация в
      Curve25519, BIP39-fingerprint (8 слов)
- [x] Вывод адреса сверен бит-в-бит с yggdrasil-go v0.5.13 (регрессионный тест +
      `scripts/validate-ygg-addr.sh`)
- [x] FFI-обёртка Yggdrasil: libygg.so (c-shared) ← crate `p2p-ygg`. Узел без
      TUN/root, датаграммы по ed25519-ключу. Loopback-тест: два узла в процессе
      обмениваются через настоящий Yggdrasil-mesh (`scripts/run-loopback.sh`)
- [ ] Управление узлом: пиринг с публичной инфраструктурой Yggdrasil, сохранение
      идентичности, переподключения
- [ ] Протокол сообщений поверх транспорта + E2E (NaCl Box)
- [ ] E2E-шифрование сообщений (NaCl Box)
- [ ] Добавление контакта по коду/QR (TOFU)
- [ ] Desktop UI (Tauri)
- [ ] Android (JNI-мост к ядру)
- [ ] Голос / видео / шаринг экрана
- [ ] Группы (MLS) / торрент-стриминг

## Разработка

```sh
cargo test -p p2p-core                   # тесты ядра
cargo run -p p2p-core --example whoami   # сгенерировать идентичность
bash scripts/validate-ygg-addr.sh        # (WSL) сверить адрес с yggdrasil-go
```

### Тулчейн

- **Rust** 1.95+ (`cargo`) — ядро, примеры, тесты.
- **Go** 1.24+ — сборка Yggdrasil-обёртки. Ставится в WSL/Linux
  (`yggdrasil-go` + cgo собираются на Linux без возни с mingw/wintun).
  Сейчас Yggdrasil-часть разрабатывается под Linux-таргет; Windows-упаковка —
  отдельная задача на этапе релиза.

## TODO / валидация

- Вывод адреса Yggdrasil (`addr_for_key`) портирован из yggdrasil-go и покрыт
  тестами на детерминизм/префикс. Нужно кросс-проверить с реальным узлом
  Yggdrasil на этапе интеграции — сверить, что адрес совпадает с тем, что
  сообщает сам `yggdrasil` для того же ключа.
