# Подпись APK — одна точка для scripts/build-apk.sh и scripts/release.sh (source, не запуск).
#
#   sign_apk <неподписанный.apk> <выход.apk> [release]
#
# Ключ — PEERAGE_KEYSTORE (+ PEERAGE_KEY_ALIAS, по умолчанию peerage) из scripts/local.env;
# пароль спрашивает apksigner, ни в окружение, ни в историю он не попадает. Без
# PEERAGE_KEYSTORE — debug-ключ Android SDK, кроме режима `release`: релиз debug-ключом
# не подписывается никогда. После подписи в SIGN_CERT_SHA256 — отпечаток сертификата
# (64 hex, без двоеточий): его публикуют вместе с релизом.
#
# v4-подпись (.idsig рядом с APK) выключена: она нужна только потоковой установке
# через adb, а при ручной установке и в релизе лишняя.

sign_windows=false
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) sign_windows=true ;; esac

# Путь к apksigner из самой свежей build-tools.
apksigner_path() {
    local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
    if [ -z "$sdk" ]; then
        if $sign_windows; then sdk="$(cygpath "$LOCALAPPDATA")/Android/Sdk"; else sdk="$HOME/Android/Sdk"; fi
    fi
    local tools
    tools="$(ls -d "$sdk"/build-tools/*/ 2>/dev/null | sort -V | tail -1)"
    local signer="${tools}apksigner"
    $sign_windows && signer="${signer}.bat"
    [ -f "$signer" ] || { echo "нет apksigner в $sdk/build-tools (задай ANDROID_HOME)" >&2; return 1; }
    printf '%s\n' "$signer"
}

sign_apk() {
    local in="$1" out="$2" mode="${3:-}"
    local signer ks alias
    signer="$(apksigner_path)" || return 1
    local pass=()
    if [ -n "${PEERAGE_KEYSTORE:-}" ]; then
        ks="$PEERAGE_KEYSTORE"
        alias="${PEERAGE_KEY_ALIAS:-peerage}"
        echo "ключ: релизный ($alias)"
    elif [ "$mode" = release ]; then
        echo "релиз подписывается только релизным ключом: PEERAGE_KEYSTORE в scripts/local.env" >&2
        return 1
    else
        ks="$HOME/.android/debug.keystore"
        alias=androiddebugkey
        pass=(--ks-pass pass:android --key-pass pass:android)
        echo "ключ: debug (релизный — PEERAGE_KEYSTORE в scripts/local.env)"
    fi
    [ -f "$ks" ] || { echo "нет ключа $ks" >&2; return 1; }
    $sign_windows && ks="$(cygpath -w "$ks")"
    "$signer" sign --v4-signing-enabled false --ks "$ks" --ks-key-alias "$alias" "${pass[@]}" \
        --out "$out" "$in" || return 1
    SIGN_CERT_SHA256="$("$signer" verify --print-certs "$out" |
        sed -n 's/.*certificate SHA-256 digest: *//p' | head -1 | tr -d ' :\r' | tr 'A-F' 'a-f')"
    [ ${#SIGN_CERT_SHA256} -eq 64 ] || { echo "не прочитан отпечаток сертификата $out" >&2; return 1; }
}
