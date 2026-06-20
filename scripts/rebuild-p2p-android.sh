#!/usr/bin/env bash
# Быстрая пересборка только libp2p_android.so (без libygg.so/Go).
# Использовать когда менялся только Rust-код (androidjni/).
# ABI: arm64-v8a (по умолч.), x86_64 для эмулятора.
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NDK="${ANDROID_NDK_HOME:-$HOME/android-ndk-r28c}"
export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export CMAKE_GENERATOR="Ninja"

ABI="${1:-arm64-v8a}"
echo "== Rebuild libp2p_android.so ($ABI) =="
find target -type d -name build -path '*audiopus_sys*' -exec rm -rf {} + 2>/dev/null || true
cargo ndk -t "$ABI" -o /tmp/p2p-jnilibs build -p p2p-android --release

dst="android/app/src/main/jniLibs/$ABI"
mkdir -p "$dst"
cp "/tmp/p2p-jnilibs/$ABI/libp2p_android.so" "$dst/"
ls -lh "$dst/libp2p_android.so"
echo "Done."
