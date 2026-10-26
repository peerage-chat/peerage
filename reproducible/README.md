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
| SDK platform 34, build-tools 34.0.0 | `sdkmanager` (сверяет sha1 по манифесту Google) |
| Gradle 8.9 | `distributionSha256Sum` в `gradle-wrapper.properties` |
| Зависимости Rust / Go / Maven | `Cargo.lock`, `go.sum`, версии в `build.gradle.kts` |

Сборка идёт из `git clone` коммита — незакоммиченные правки и локальные `.so` в неё не
попадают. Время сборки = время коммита (`SOURCE_DATE_EPOCH`), путь — всегда
`/build/p2p`, пути в отладочной информации переписаны на постоянные имена.

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
