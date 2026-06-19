#!/bin/sh
export PATH="/usr/local/go/bin:/home/fixman/.cargo/bin:/usr/bin:/bin:$PATH"
NDK="/home/fixman/android-ndk-r28c"
ROOT="/mnt/c/Users/fixman/Documents/projects/p2p"
cd "$ROOT" || exit 1

export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export CMAKE_GENERATOR="Ninja"

echo "==> Building arm64-v8a (Rust)"
cargo ndk -t arm64-v8a -o /tmp/p2p-jnilibs build -p p2p-android --release 2>&1
/bin/cp /tmp/p2p-jnilibs/arm64-v8a/libp2p_android.so android/app/src/main/jniLibs/arm64-v8a/
echo "OK: arm64-v8a"

echo "==> Building x86_64 (Rust)"
cargo ndk -t x86_64 -o /tmp/p2p-jnilibs build -p p2p-android --release 2>&1
/bin/cp /tmp/p2p-jnilibs/x86_64/libp2p_android.so android/app/src/main/jniLibs/x86_64/
echo "OK: x86_64"

echo "=== Rust done ==="
