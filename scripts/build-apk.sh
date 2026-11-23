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
#   3. Подпись (scripts/lib/sign.sh): release без signingConfig не ставится. Ключ в gradle не прописан
#      нарочно — ни путь, ни пароль не попадают в репозиторий, а неподписанный APK
#      остаётся тем же, что даёт воспроизводимая сборка (reproducible/README.md).
#      * Релизный ключ: PEERAGE_KEYSTORE (и PEERAGE_KEY_ALIAS, по умолчанию peerage) —
#        в scripts/local.env этой машины. Пароль спрашивает apksigner: ни в окружение,
#        ни в историю команд он не попадает. В конце — отпечаток сертификата и SHA-256
#        файла: их публикуют вместе с релизом.
#      * Без PEERAGE_KEYSTORE — debug-ключ Android SDK (~/.android/debug.keystore).
#      Релиз с другим ключом поверх прежней сборки не встаёт — только удалить и
#      поставить заново. Раздаваемый релиз собирает не этот скрипт, а scripts/release.sh
#      (воспроизводимая сборка тега, docs/release.md).
# Установка — не здесь: APK копируется на телефон и ставится вручную (adb не нужен).
#
# Нужны: тулчейн из android/README.md (NDK, cargo-ndk, Go), Android SDK (build-tools)
# и JDK 17+ (JAVA_HOME; на Windows подойдёт и встроенный в Android Studio).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# Машинные настройки (путь к релизному ключу и т.п.) — не в git.
[ -f scripts/local.env ] && . scripts/local.env
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
. scripts/lib/sign.sh
sign_apk "$unsigned" "$OUT"
echo "Отпечаток сертификата (SHA-256): $SIGN_CERT_SHA256"
sha256sum "$OUT"
ls -lh "$OUT"
echo "Готово: $OUT — скопировать на телефон и установить."
