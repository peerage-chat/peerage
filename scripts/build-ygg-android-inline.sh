#!/bin/sh
export PATH="/usr/local/go/bin:/usr/bin:/bin:$PATH"
NDK="/home/fixman/android-ndk-r28c"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
API=24
ROOT="$(cd "$(dirname "$0")/../ygg" && pwd)"
cd "$ROOT" || exit 1

build_abi() {
    ABI=$1
    case $ABI in
        arm64-v8a) GOARCH=arm64; CC="$TOOLCHAIN/aarch64-linux-android${API}-clang" ;;
        x86_64)    GOARCH=amd64; CC="$TOOLCHAIN/x86_64-linux-android${API}-clang" ;;
        *) echo "Unknown ABI: $ABI"; exit 1 ;;
    esac
    mkdir -p "libygg/android/$ABI"
    echo "==> Building $ABI (GOARCH=$GOARCH)"
    CGO_ENABLED=1 GOOS=android GOARCH="$GOARCH" CC="$CC" \
        go build -buildmode=c-shared -o "libygg/android/$ABI/libygg.so" ./libygg
    ls -la "libygg/android/$ABI/libygg.so"
    echo "OK: $ABI"
}

build_abi arm64-v8a
build_abi x86_64
echo "=== libygg done ==="
