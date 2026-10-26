#!/bin/sh
export PATH="/usr/local/go/bin:/home/fixman/.cargo/bin:/usr/bin:/bin:$PATH"
NDK="/home/fixman/android-ndk-r28c"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export CMAKE_GENERATOR="Ninja"

echo "==> Building arm64-v8a (Rust)"
cargo ndk -t arm64-v8a -o /tmp/jnilibs build -p peerage-android --release 2>&1
/bin/cp /tmp/jnilibs/arm64-v8a/libpeerage_android.so android/app/src/main/jniLibs/arm64-v8a/
echo "OK: arm64-v8a"

echo "==> Building x86_64 (Rust)"
cargo ndk -t x86_64 -o /tmp/jnilibs build -p peerage-android --release 2>&1
/bin/cp /tmp/jnilibs/x86_64/libpeerage_android.so android/app/src/main/jniLibs/x86_64/
echo "OK: x86_64"

echo "=== Rust done ==="
