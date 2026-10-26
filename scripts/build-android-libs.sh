#!/usr/bin/env bash
# Собрать обе нативные библиотеки под Android и разложить в jniLibs приложения.
#   1) libygg.so   — Go c-shared (yggdrasil) через Linux-NDK
#   2) libpeerage_android.so — Rust cdylib (ядро+мессенджер+JNI) через cargo-ndk
# Результат: android/app/src/main/jniLibs/<ABI>/{libygg.so,libpeerage_android.so}
#
# Требуется (см. android/README.md):
#   - ANDROID_NDK_HOME → Linux-NDK (для Go cgo)
#   - rustup target add aarch64-linux-android && cargo install cargo-ndk
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# Машинные настройки (напр. CARGO_TARGET_DIR на другом диске) — в scripts/local.env,
# он в .gitignore. Без него сборка идёт в ./target, как обычно.
[ -f scripts/local.env ] && . scripts/local.env
T="${CARGO_TARGET_DIR:-target}"

# NDK: из окружения, иначе — распакованный setup-android-ndk.sh в $HOME.
NDK="${ANDROID_NDK_HOME:-$HOME/android-ndk-r28c}"
export ANDROID_NDK_HOME="$NDK"
[ -d "$NDK" ] || { echo "нет NDK: $NDK (см. scripts/setup-android-ndk.sh)" >&2; exit 1; }
ABI="${1:-arm64-v8a}"
declare -A RUST_TARGET=(
    [arm64-v8a]=aarch64-linux-android
    [armeabi-v7a]=armv7-linux-androideabi
    [x86_64]=x86_64-linux-android
    [x86]=i686-linux-android
)
target="${RUST_TARGET[$ABI]:?неизвестный ABI: $ABI}"

echo "== 1/3 libygg ($ABI) =="
bash scripts/build-ygg-android.sh "$ABI"

echo "== 2/3 libpeerage_android ($ABI) =="
rustup target add "$target" >/dev/null 2>&1 || true
# Кросс-компиляция C/C++-зависимостей (opus через cmake, openh264 через cc).
# Используем ВСТРОЕННУЮ поддержку Android в cmake: она находит NDK по
# ANDROID_NDK_ROOT, а ABI выводит из CMAKE_SYSTEM_PROCESSOR (его ставит cargo-ndk).
# Toolchain-файл NDK НЕ подключаем — он конфликтует с флагами cargo-ndk (ABI).
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export CMAKE_GENERATOR="Ninja"
# Чистим возможный stale CMakeCache от предыдущей сборки (смена генератора/ABI).
find "$T/$target" -type d -name build -path '*audiopus_sys*' -exec rm -rf {} + 2>/dev/null || true
cargo ndk -t "$ABI" -o /tmp/jnilibs build -p peerage-android --release

echo "== 3/3 стейджинг в jniLibs =="
dst="android/app/src/main/jniLibs/$ABI"
mkdir -p "$dst"
cp "ygg/libygg/android/$ABI/libygg.so" "$dst/"
cp "/tmp/jnilibs/$ABI/libpeerage_android.so" "$dst/"
# libc++_shared.so из NDK — нужна libpeerage_android.so (C++ openh264).
TRIPLE_DIR="aarch64-linux-android"
case "$ABI" in
    armeabi-v7a) TRIPLE_DIR="arm-linux-androideabi" ;;
    x86_64) TRIPLE_DIR="x86_64-linux-android" ;;
    x86) TRIPLE_DIR="i686-linux-android" ;;
esac
SYSROOT_LIB="$NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$TRIPLE_DIR"
cp "$SYSROOT_LIB/libc++_shared.so" "$dst/"
ls -la "$dst"
echo "готово."
