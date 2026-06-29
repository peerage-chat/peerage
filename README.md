# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полный P2P, без сервера — связь через mesh-сеть Yggdrasil.

## Документация → `docs/`

| Документ | Описание |
|----------|----------|
| [docs/README.md](docs/README.md) | **← индекс-хаб:** routing по всем темам |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | **Сквозная картина системы** end-to-end (крейты, крипто, транспорт, медиа, Android, сборка) |
| [docs/architecture/invariants.md](docs/architecture/invariants.md) | Сквозные правила (FFI/каналы/SQLite/MLS/UI) — не нарушать |
| [docs/SECURITY.md](docs/SECURITY.md) | Модель угроз, крипто, аудит безопасности |
| [docs/ROADMAP.md](docs/ROADMAP.md) · [docs/v1.0-scope.md](docs/v1.0-scope.md) | Порядок работ и скоуп v1.0 |

## Быстрый старт

```sh
cargo test -p p2p-core                   # тесты ядра
cargo run -p p2p-core --example whoami   # сгенерировать идентичность
bash scripts/run-chat.sh                 # (WSL) E2E чат по localhost
```

## Лицензия

[GPL-3.0-only](LICENSE)
