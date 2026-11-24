# Воспроизводимая сборка

Цель: любой может собрать APK из исходников и убедиться, что он **побайтно** совпадает
с тем, что мы раздаём. Тогда доверять приходится коду, который можно прочитать, а не
машине, на которой собирали.

## Что закреплено

| Что | Как |
|---|---|
| Базовый образ | `debian:bookworm-slim` по digest |
| Пакеты Debian (JDK 17, cmake, ninja…) | снимок `snapshot.debian.org` на дату (`DEBIAN_SNAPSHOT`) |
| Go 1.26.4, rustup 1.28.2, Rust 1.96.0, NDK r28c, cmdline-tools | версия + sha256 архива в `Dockerfile` |
| SDK platform 37.2, build-tools 36.1.0 | `sdkmanager` (сверяет sha1 по манифесту Google); те же версии — в `android/app/build.gradle.kts` (`compileSdk`, `buildToolsVersion`) |
| Gradle 9.9 | `distributionSha256Sum` в `gradle-wrapper.properties` |
| Зависимости Rust / Go / Maven | `Cargo.lock`, `go.sum`, `android/gradle/verification-metadata.xml` — все с хэшами |
| platform-tools | **не закреплено**: sdkmanager отдаёт только последнюю; в APK не попадает |

Сборка идёт из `git clone` коммита — незакоммиченные правки и локальные `.so` в неё не
попадают. Время сборки = время коммита (`SOURCE_DATE_EPOCH`), путь — всегда
`/build/src`, пути в отладочной информации переписаны на постоянные имена.

## Что держит сборку побайтной

Всё ниже уже сделано; ломается при обновлении зависимостей — тогда проверять заново.

- **R8 из AGP 9.5 недетерминирован** в `kotlinx.coroutines.flow.FlowKt__LimitKt`
  (`emitAbort` и аксессор): то сужает тип параметра, то нет. Сигнатуры закреплены
  правилом в `android/app/proguard-rules.pro`. После смены AGP/R8 правила может не хватить:
  сравнивать дампы dex двух сборок (`dexdump` из build-tools).
- **Хэши Maven-зависимостей пишутся на Linux не все.** Перегенерация
  `verification-metadata.xml` на Windows пропускает BOM из кэша (нужен
  `--refresh-dependencies`) и `aapt2-<версия>-linux.jar` (дописывается руками: sha1 сверить с
  Google Maven, sha256 посчитать). Иначе Docker-сборка падает на проверке хэшей.
- **Версии SDK — в двух местах**: `build.gradle.kts` и `Dockerfile` (`sdkmanager`). Меняются
  вместе.
- **WSL**: при скачке часов `openssl-src` иногда падает с «file in the future» — сборка
  ничего не портит, повторить.

## Как проверить

Нужны Linux (или WSL2) и Docker.

```bash
scripts/repro-build.sh --check          # две чистые сборки HEAD и сравнение хэшей
scripts/repro-build.sh out v0.1.0       # одна сборка тега в каталог out/
```

В каталоге вывода: `app-arm64-v8a-release-unsigned.apk`, три `.so`, `COMMIT` и
`SHA256SUMS`.

**Сверка с подписанным релизом.** Раздаваемый APK подписан, поэтому его хэш с
неподписанным не совпадёт. Сравнивается всё, кроме подписи: например,
[`apksigcopier compare`](https://github.com/obfusk/apksigcopier) — именно так F-Droid
сверяет воспроизводимые сборки.

## Лицензия Android SDK

`android-sdk-licenses/` — хэш лицензии SDK, которую владелец проекта уже принял в
своём SDK. Собирая образ, вы пользуетесь Android SDK на её условиях
(<https://developer.android.com/studio/terms>).
