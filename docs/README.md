# P2P Messenger — документация

Децентрализованный P2P-мессенджер с E2E-шифрованием, голосом, видео и шарингом
экрана. Полный P2P, без сервера — связь через mesh-сеть **Yggdrasil**. Единое
Rust-ядро; платформы (Android, десктоп) — тонкие адаптеры. GPL-3.0-only.

Это **индекс-хаб**: routing по темам. Сквозная картина системы — в
**[ARCHITECTURE.md](ARCHITECTURE.md)** (читать первым, если разбираешься с проектом).

## Куда смотреть

| Нужно | Документ |
|---|---|
| **Понять систему end-to-end** | [ARCHITECTURE.md](ARCHITECTURE.md) — крейты, потоки данных, транспорт, крипто, медиа, Android, сборка |
| **Сквозные правила** (не нарушать) | [architecture/invariants.md](architecture/invariants.md) — FFI/каналы/SQLite/MLS/UI |
| **Подсистемы → фичи** | [SUBSYSTEMS.md](SUBSYSTEMS.md) — концептуальный хребет |
| **Контракт движок↔клиент** | [protocol-spec.md](protocol-spec.md), [adr/](adr/) (архитектурные решения) |
| **Безопасность / модель угроз** | [SECURITY.md](SECURITY.md), [architecture/threat-model.md](architecture/threat-model.md) (активы/противники/гарантии), [architecture/security-audit-2026-07-02.md](architecture/security-audit-2026-07-02.md) (крипто-аудит) |
| **UI** | [design/app-ui.md](design/app-ui.md) (язык, ИСТОЧНИК ИСТИНЫ), [design/call-ui.md](design/call-ui.md) (звонки), [design/tokens.md](design/tokens.md) (цвета/отступы) |
| **Что и в каком порядке строим** | [v1.0-scope.md](v1.0-scope.md), [completion-plan.md](completion-plan.md), [ROADMAP.md](ROADMAP.md) |
| **Планы развития** | [plans/media-webrtc-parity.md](plans/media-webrtc-parity.md), [plans/security-hardening.md](plans/security-hardening.md) |
| **Известные баги** | [BUGS.md](BUGS.md) |
| **История / исходное ТЗ** | [archive/](archive/) |

## Быстрый старт

```sh
# Ядро (любая платформа):
cargo test -p p2p-core
cargo run -p p2p-core --example whoami        # сгенерировать идентичность

# Сетевые тесты / десктоп (через WSL):
bash scripts/run-chat.sh                      # E2E чат по localhost
bash scripts/run-realnet-chat.sh              # E2E чат двух узлов через интернет
bash scripts/run-desktop.sh                   # десктоп-приложение (Slint, через WSLg)

# Android:
bash scripts/rebuild-p2p-android.sh x86_64    # Rust-only пересборка (.so), эмулятор
bash scripts/build-android-libs.sh x86_64     # полная (когда менялся ygg.go FFI)
./gradlew :app:assembleDebug                  # APK
./gradlew ktlintFormat                        # перед коммитом Kotlin
```

Подробности сборки/тулчейна — [ARCHITECTURE.md §10](ARCHITECTURE.md#10-сборка-и-разработка).
Публичные пиры (`peers/public-peers.txt`) устаревают — актуальные:
<https://publicpeers.neilalexander.dev/>.

## Тулчейн

Rust 1.95+ · Go 1.24+ (libygg, WSL) · Android NDK r28c + cargo-ndk · Gradle 8.9 + JDK 21.

## Лицензия

[GPL-3.0-only](../LICENSE). Используется GPL-вариант три-лицензии Slint — всё приложение
копилефтное, исходники открыты и проверяемы.
