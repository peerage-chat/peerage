# Peerage (P2P-мессенджер) — рабочие правила

Serverless E2E-мессенджер: Yggdrasil-меш, MLS-группы (openmls), Rust-ядро,
Android Jetpack Compose. GPL-3.0. Цель — v1.0 (на гранты не подаёмся).

## Каноничные доки (читать перед правкой соответствующей зоны)

- **`docs/architecture/invariants.md`** — сквозные инварианты (FFI/каналы/SQLite/MLS/
  lifecycle). **Обязательны к соблюдению.**
- **`docs/design/tokens.md`** + `Tokens.kt` — дизайн-токены (цвета/отступы/размеры).
- **`docs/design/app-ui.md`** — язык/навигация всего приложения (ИСТОЧНИК ИСТИНЫ по UI).
- **`docs/design/notifications.md`** — уведомления (источник истины, раздел «Итог»).
- **`docs/design/call-ui.md`** — экраны звонка. Эталон — макеты юзера; `*-ui.md` вторичен.
- **`docs/BRAND.md`** — имя продукта: что от него зависит, что заморожено (метки протокола);
  сменить — `scripts/rebrand.py`. Видимое имя в коде — только `R.string.app_name`.
- **`docs/architecture/engine-changelog.md`** — журнал изменений движка (контракт, схема,
  поведение). **Любая правка `core`/`messenger`/`engine`/`androidjni`/`ygg`, меняющая
  контракт или поведение, — с записью там в том же коммите**, с причиной (план/ADR/R-NNN/баг).
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
  (`scripts/rebuild-android.sh <ABI>`; при изменении ygg.go FFI — `build-android-libs.sh`).
- Коммиты — по-русски, с `Co-Authored-By`. **Коммитить и пушить в master — всегда, без
  спроса** (перекрывает правило ub-mode «не пушить, спросить»). Force-push и переписывание
  истории — только с явного разрешения.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.
