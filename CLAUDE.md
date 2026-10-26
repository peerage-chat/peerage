# Peerage (P2P-мессенджер) — рабочие правила

Serverless E2E-мессенджер: Yggdrasil-меш, MLS-группы (openmls), Rust-ядро,
Android Jetpack Compose. GPL-3.0. Цель — v1.0 под грант NLnet/NGI Zero.

## Каноничные доки (читать перед правкой соответствующей зоны)

- **`docs/architecture/invariants.md`** — сквозные инварианты (FFI/каналы/SQLite/MLS/
  lifecycle). **Обязательны к соблюдению.**
- **`docs/design/tokens.md`** + `Tokens.kt` — дизайн-токены (цвета/отступы/размеры).
- **`docs/design/app-ui.md`** — язык/навигация всего приложения (ИСТОЧНИК ИСТИНЫ по UI).
- **`docs/design/call-ui.md`** — экраны звонка. Эталон — макеты юзера; `*-ui.md` вторичен.
- **`docs/BRAND.md`** — имя продукта: что от него зависит, что заморожено (метки протокола);
  сменить — `scripts/rebrand.py`. Видимое имя в коде — только `R.string.app_name`.
- `docs/SUBSYSTEMS.md`, `docs/adr/` — крейты и архитектурные решения. `docs/v1.0-scope.md`,
  `docs/completion-plan.md` — скоуп и последовательность.

## Незыблемые инварианты (детали — invariants.md)

1. **FFI/JNI:** каждая точка входа JNI — через `catch_unwind`-макрос (`androidjni/src/lib.rs`).
   Паника на границе = краш всего процесса. Никаких новых `unwrap()` на fallible.
2. **Каналы:** `CHAN_CHAT` (0x01, надёжный, ACK) — чат/ключи/управление; `CHAN_MEDIA`
   (0x02, без ACK) — только аудио/видео. НЕ смешивать.
3. **MLS:** любая операция, двигающая состояние (send/receive/commit), ОБЯЗАНА `save_group`.
4. **UI:** Compose НИКОГДА не зовёт `Native.*` напрямую — только через фоновые потоки
   (`pollThread`/`tickerThread`/`ops`); состояние — snapshot-state.
5. **UI-токены:** новый код — на `Tokens.*`, не на инлайн-хекс/магические dp.
6. **Файлы состояния — только атомарно:** `peerage_core::fsutil::write_atomic` (ключи/сиды —
   `write_atomic_private`), не голый `fs::write`: Android убивает процесс посреди записи.

## Процесс

- **Есть точные макеты — реализуй по ним 1-в-1**, не по текстовому описанию.
- **Однодевайсные вещи** (цвет/размер/раскладка) проверяй сам скриншотом эмулятора
  перед «готово»; двух-девайсный тест — только для сетевых/медийных.
- Rust-тесты: `scripts/test-all.sh` из WSL (сам ставит `--test-threads=1`,
  `LD_LIBRARY_PATH` и исключает несобираемый desktop).
- `./gradlew ktlintFormat` перед коммитом Kotlin. Rust-части Android — через WSL
  (`scripts/rebuild-peerage-android.sh <ABI>`; при изменении ygg.go FFI — `build-android-libs.sh`).
- Коммиты — по-русски, с `Co-Authored-By`. Коммитить/пушить только по просьбе.
