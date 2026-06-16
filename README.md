# P2P Messenger

Децентрализованный P2P мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полный P2P, без сервера — связь через mesh-сеть Yggdrasil.

## Документация → `docs/`

| Документ | Описание |
|----------|----------|
| [docs/README.md](docs/README.md) | **← основная страница:** архитектура, структура проекта, прогресс, разработка |
| [docs/architecture-review.md](docs/architecture-review.md) | Архитектурный анализ, проблемы масштабирования, рекомендации |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Дорожная карта: фазы, прогресс, зависимости |
| [docs/SECURITY.md](docs/SECURITY.md) | Модель угроз, крипто, аудит безопасности |
| [docs/p2p-messenger-spec.md](docs/p2p-messenger-spec.md) | Исходное техническое задание |

## Быстрый старт

```sh
cargo test -p p2p-core                   # тесты ядра
cargo run -p p2p-core --example whoami   # сгенерировать идентичность
bash scripts/run-chat.sh                 # (WSL) E2E чат по localhost
```

## Лицензия

[GPL-3.0-only](LICENSE)
