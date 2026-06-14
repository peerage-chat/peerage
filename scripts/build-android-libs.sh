#!/usr/bin/env bash
# Собрать обе нативные библиотеки под Android и разложить в jniLibs приложения.
#   1) libygg.so   — Go c-shared (yggdrasil) через Linux-NDK
#   2) libp2p_android.so — Rust cdylib (ядро+мессенджер+JNI) через cargo-ndk
# Результат: android/app/src/main/jniLibs/<ABI>/{libygg.so,libp2p_android.so}
#
# Требуется (см. android/README.md):
#   - ANDROID_NDK_HOME → Linux-NDK (для Go cgo)
#   - rustup target add aarch64-linux-android && cargo install cargo-ndk
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

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

echo "== 2/3 libp2p_android ($ABI) =="
rustup target add "$target" >/dev/null 2>&1 || true
cargo ndk -t "$ABI" -o /tmp/p2p-jnilibs build -p p2p-android --release

echo "== 3/3 стейджинг в jniLibs =="
dst="android/app/src/main/jniLibs/$ABI"
mkdir -p "$dst"
cp "ygg/libygg/android/$ABI/libygg.so" "$dst/"
cp "/tmp/p2p-jnilibs/$ABI/libp2p_android.so" "$dst/"
ls -la "$dst"
echo "готово."
