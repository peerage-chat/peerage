#!/usr/bin/env bash
# Собрать подписанный APK одной командой — из Git Bash на Windows или из Linux:
#   scripts/build-apk.sh              — телефон (arm64-v8a) → peerage-arm64.apk
#   scripts/build-apk.sh x86_64       — эмулятор             → peerage-x86_64.apk
#
# Шаги и почему в таком порядке:
#   1. Нативные библиотеки — ОБЕ (libygg.so из Go и libpeerage_android.so из Rust),
#      заново, для этого ABI. Только Rust недостаточно: свежая libpeerage ссылается на
#      символы libygg, которых в старой может не быть, — вылет на запуске
#      (`dlopen failed: cannot locate symbol`). Собираются под Linux (Go cgo требует
#      Linux-NDK); на Windows — через WSL (scripts/w).
#   2. APK (gradle assembleRelease) — после библиотек: gradle кладёт в APK то, что
#      лежит в jniLibs на момент сборки.
#   3. Подпись: release без signingConfig не ставится. По умолчанию — debug-ключ
#      Android SDK (~/.android/debug.keystore): обновление ставится поверх прежней
#      сборки, пока ключ тот же. Свой ключ — PEERAGE_KEYSTORE, PEERAGE_KS_PASS,
#      PEERAGE_KEY_ALIAS, PEERAGE_KEY_PASS.
# Установка — не здесь: APK копируется на телефон и ставится вручную (adb не нужен).
#
# Нужны: тулчейн из android/README.md (NDK, cargo-ndk, Go), Android SDK (build-tools)
# и JDK 17+ (JAVA_HOME; на Windows подойдёт и встроенный в Android Studio).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
ABI="${1:-arm64-v8a}"
case "$ABI" in
arm64-v8a) OUT="peerage-arm64.apk" ;;
x86_64) OUT="peerage-x86_64.apk" ;;
*) echo "ABI: arm64-v8a или x86_64 (их собирает gradle, см. splits)" >&2; exit 2 ;;
esac

windows=false
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) windows=true ;; esac

echo "== 1/3 нативные библиотеки ($ABI) =="
if $windows; then
    scripts/w bash scripts/build-android-libs.sh "$ABI"
else
    bash scripts/build-android-libs.sh "$ABI"
fi

echo "== 2/3 APK =="
if [ -z "${JAVA_HOME:-}" ] && $windows && [ -d "/c/Program Files/Android/Android Studio/jbr" ]; then
    export JAVA_HOME="C:\\Program Files\\Android\\Android Studio\\jbr"
fi
(cd android && ./gradlew --quiet :app:assembleRelease)
unsigned="android/app/build/outputs/apk/release/app-$ABI-release-unsigned.apk"
[ -f "$unsigned" ] || { echo "нет $unsigned" >&2; exit 1; }

echo "== 3/3 подпись =="
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [ -z "$SDK" ]; then
    if $windows; then SDK="$(cygpath "$LOCALAPPDATA")/Android/Sdk"; else SDK="$HOME/Android/Sdk"; fi
fi
tools="$(ls -d "$SDK"/build-tools/*/ 2>/dev/null | sort -V | tail -1)"
signer="${tools}apksigner"
$windows && signer="${signer}.bat"
[ -f "$signer" ] || { echo "нет apksigner в $SDK/build-tools (задай ANDROID_HOME)" >&2; exit 1; }
ks="${PEERAGE_KEYSTORE:-$HOME/.android/debug.keystore}"
[ -f "$ks" ] || { echo "нет ключа $ks (появится после любой debug-сборки в Android Studio)" >&2; exit 1; }
$windows && ks="$(cygpath -w "$ks")"
# v4 (.idsig рядом с APK) — для потоковой установки через adb; при ручной не нужна.
"$signer" sign --v4-signing-enabled false --ks "$ks" \
    --ks-pass "pass:${PEERAGE_KS_PASS:-android}" \
    --ks-key-alias "${PEERAGE_KEY_ALIAS:-androiddebugkey}" \
    --key-pass "pass:${PEERAGE_KEY_PASS:-android}" \
    --out "$OUT" "$unsigned"
"$signer" verify "$OUT"
ls -lh "$OUT"
echo "Готово: $OUT — скопировать на телефон и установить."
