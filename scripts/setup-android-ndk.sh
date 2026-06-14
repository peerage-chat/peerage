#!/usr/bin/env bash
# Скачать и распаковать Linux-NDK в $HOME (для Go cgo при сборке libygg).
# Печатает строку ANDROID_NDK_HOME=<path> в конце.
set -euo pipefail
cd "$HOME"

CANDIDATES="android-ndk-r28c-linux.zip android-ndk-r28b-linux.zip android-ndk-r27c-linux.zip android-ndk-r26d-linux.zip"
base="https://dl.google.com/android/repository"

zip=""
for c in $CANDIDATES; do
    url="$base/$c"
    code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 30 -I "$url" || echo 000)
    echo "probe $c -> $code"
    if [ "$code" = "200" ]; then zip="$c"; break; fi
done
[ -n "$zip" ] || { echo "не нашёл доступный NDK-zip" >&2; exit 1; }

dir="${zip%-linux.zip}"
if [ -d "$HOME/$dir" ]; then
    echo "уже распакован: $HOME/$dir"
else
    echo "качаю $zip ..."
    curl -fL --max-time 1200 -o "$zip" "$base/$zip"
    echo "распаковываю ..."
    unzip -q -o "$zip"
    rm -f "$zip"
fi
echo "ANDROID_NDK_HOME=$HOME/$dir"
