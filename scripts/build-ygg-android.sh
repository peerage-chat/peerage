#!/usr/bin/env bash
# Кросс-компиляция libygg.so (c-shared обёртка yggdrasil-go) под Android ABI.
# Кладёт по одной .so на ABI в ygg/libygg/android/<ABI>/ — оттуда их забирают
# и build.rs транспорта (при линковке androidjni), и Gradle (в jniLibs).
#
# Требуется Linux-NDK в WSL (Windows-NDK из SDK не годится для linux-Go cgo).
# Установка NDK r29 в WSL (один раз):
#   cd ~ && wget https://dl.google.com/android/repository/android-ndk-r29-linux.zip
#   unzip -q android-ndk-r29-linux.zip
#   export ANDROID_NDK_HOME=$HOME/android-ndk-r29
#
# Запуск: ANDROID_NDK_HOME=/path/to/ndk bash scripts/build-ygg-android.sh [ABI...]
set -euo pipefail
export PATH="/usr/local/go/bin:$PATH"
export CGO_ENABLED=1

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/ygg"

NDK="${ANDROID_NDK_HOME:?установите ANDROID_NDK_HOME на Linux-NDK (см. шапку скрипта)}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
API="${ANDROID_API:-24}" # минимальный API проекта

# ABI → (GOARCH, имя clang-обёртки)
declare -A GOARCH=( [arm64-v8a]=arm64 [armeabi-v7a]=arm [x86_64]=amd64 [x86]=386 )
declare -A CLANG=(
    [arm64-v8a]="aarch64-linux-android${API}-clang"
    [armeabi-v7a]="armv7a-linux-androideabi${API}-clang"
    [x86_64]="x86_64-linux-android${API}-clang"
    [x86]="i686-linux-android${API}-clang"
)

ABIS=("$@")
[ ${#ABIS[@]} -eq 0 ] && ABIS=(arm64-v8a) # по умолчанию — только arm64 (реальные устройства)

# В воспроизводимой сборке (REPRO=1) зависимости только из go.sum: tidy мог бы
# молча подтянуть другую версию.
if [ -n "${REPRO:-}" ]; then
    export GOFLAGS="-mod=readonly"
else
    go mod tidy >/dev/null 2>&1
fi
for abi in "${ABIS[@]}"; do
    cc="$TOOLCHAIN/${CLANG[$abi]}"
    [ -x "$cc" ] || { echo "нет компилятора $cc — проверьте NDK" >&2; exit 1; }
    out="libygg/android/$abi"
    mkdir -p "$out"
    echo "→ $abi (GOARCH=${GOARCH[$abi]})"
    CC="$cc" GOOS=android GOARCH="${GOARCH[$abi]}" \
        # -trimpath и пустой buildid: в бинарнике нет путей машины и случайного id,
        # одинаковые исходники дают одинаковые байты (шаг 7 плана беты).
        go build -buildmode=c-shared -trimpath -buildvcs=false -ldflags=-buildid=             -o "$out/libygg.so" ./libygg
    ls -la "$out/libygg.so"
done
echo "готово: ygg/libygg/android/"
