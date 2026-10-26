#!/usr/bin/env bash
# Быстрая пересборка только libpeerage_android.so (без libygg.so/Go).
# Использовать когда менялся только Rust-код (androidjni/).
# ABI: arm64-v8a (по умолч.), x86_64 для эмулятора.
set -euo pipefail
export PATH="/usr/local/go/bin:$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# Машинные настройки (напр. CARGO_TARGET_DIR на другом диске) — в scripts/local.env,
# он в .gitignore. Без него сборка идёт в ./target, как обычно.
[ -f scripts/local.env ] && . scripts/local.env
T="${CARGO_TARGET_DIR:-target}"

NDK="${ANDROID_NDK_HOME:-$HOME/android-ndk-r28c}"
export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export CMAKE_GENERATOR="Ninja"

ABI="${1:-arm64-v8a}"
echo "== Rebuild libpeerage_android.so ($ABI) =="
find "$T" -type d -name build -path '*audiopus_sys*' -exec rm -rf {} + 2>/dev/null || true
cargo ndk -t "$ABI" -o /tmp/jnilibs build -p peerage-android --release

dst="android/app/src/main/jniLibs/$ABI"
mkdir -p "$dst"
cp "/tmp/jnilibs/$ABI/libpeerage_android.so" "$dst/"
ls -lh "$dst/libpeerage_android.so"
echo "Done."
