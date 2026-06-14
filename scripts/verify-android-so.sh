#!/usr/bin/env bash
# Проверка нативных .so: DT_NEEDED, экспорт JNI-символов, архитектура.
set -euo pipefail
NDK="${ANDROID_NDK_HOME:?}"
BIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SO="$ROOT/android/app/src/main/jniLibs/arm64-v8a/libp2p_android.so"

echo "=== NEEDED / SONAME ==="
"$BIN/llvm-readelf" -d "$SO" | grep -E "NEEDED|SONAME" || true
echo "=== JNI exports (Java_dev_p2p_messenger_Native_*) ==="
"$BIN/llvm-nm" -D "$SO" | grep "Java_dev_p2p" || echo "НЕТ JNI-символов!"
echo "=== header ==="
"$BIN/llvm-readelf" -h "$SO" | grep -E "Machine|Class"
echo "=== libygg SONAME ==="
"$BIN/llvm-readelf" -d "$ROOT/android/app/src/main/jniLibs/arm64-v8a/libygg.so" | grep -E "SONAME" || true
