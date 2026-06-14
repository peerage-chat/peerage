# Android-клиент

Compose-приложение (Holo-тема) поверх того же Rust-ядра, что и десктоп. UI на
Kotlin; вся логика (идентичность, крипто, мессенджер, MLS) — в `p2p-core`/
`p2p-messenger`, вызывается через JNI-мост `androidjni` (cdylib `libp2p_android.so`).

```
Kotlin/Compose  ──JNI──▶  libp2p_android.so (androidjni)
                              │ depends on
                              ▼
                          libygg.so (Go c-shared, Yggdrasil)
```

## Что готово (фаза G2)

- JNI-мост: идентичность, контакты (TOFU, верификация), 1:1 текст-чат, статусы
  доставки, онлайн-присутствие, история — паритет с десктоп-чатом.
- Compose-экраны: список контактов, чат, добавление, «мой код», инфо о контакте.
- Модель событий — поллинг `Native.pollEvent()`; источник истины — Rust-ядро.

Звонки (G3) и группы (G4) — следующие фазы.

## Сборка нативных библиотек

Нужны **две** .so на каждый ABI: `libygg.so` (Go) и `libp2p_android.so` (Rust).
Собираются из WSL/Linux (Go cgo требует Linux-NDK).

### Однократная настройка тулчейна (в WSL)

```bash
# 1. Linux-NDK (Windows-NDK из Android Studio не годится для Go cgo):
cd ~ && wget https://dl.google.com/android/repository/android-ndk-r28c-linux.zip
unzip -q android-ndk-r28c-linux.zip
export ANDROID_NDK_HOME=$HOME/android-ndk-r28c   # добавьте в ~/.bashrc

# 2. Rust-таргет + cargo-ndk:
rustup target add aarch64-linux-android
cargo install cargo-ndk
```

### Сборка (каждый раз)

```bash
ANDROID_NDK_HOME=$HOME/android-ndk-r28c bash scripts/build-android-libs.sh arm64-v8a
```

Скрипт собирает обе библиотеки и кладёт их в
`android/app/src/main/jniLibs/arm64-v8a/`. Для эмулятора добавьте `x86_64`.

## Сборка APK

Откройте каталог `android/` в Android Studio (нужен встроенный JDK 17 и SDK 34)
и запустите на устройстве/эмуляторе, либо из консоли:

```bash
cd android && ./gradlew assembleDebug
```

Gradle берёт готовые `.so` из `jniLibs` (см. `app/build.gradle.kts`). Нативные
библиотеки нужно собрать ДО Gradle (шаг выше).

## Статус сборки

Проверено end-to-end: нативные .so кросс-компилируются (libygg.so + libp2p_android.so,
AArch64; libp2p_android корректно линкуется с libygg, все 14 JNI-символов
экспортированы) и `gradle assembleDebug` собирает установочный `app-debug.apk`
(~16 МБ, обе .so внутри `lib/arm64-v8a/`). На реальном устройстве ещё не запускалось.

Сборка APK проверена с Gradle 8.9 + JBR от Android Studio (JDK 21).

## Версии тулчейна

AGP 8.5.2 · Kotlin 1.9.25 · Compose compiler 1.5.15 · compileSdk 34 · minSdk 24.
