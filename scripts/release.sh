#!/usr/bin/env bash
# Релиз Peerage из тега — из Git Bash на Windows или из Linux:
#   scripts/release.sh v0.1.0
#
# Что делает (порядок и форматы — docs/release.md):
#   1. Проверяет тег: он есть, версия в build.gradle.kts на нём совпадает с тегом, и
#      versionCode больше, чем у прошлого релиза (иначе обновление не встанет).
#   2. Воспроизводимая сборка ИМЕННО этого тега в Docker (scripts/repro-build.sh): что
#      раздаётся, то любой может собрать сам и сверить — reproducible/README.md.
#   3. Подпись релизным ключом (scripts/lib/sign.sh; пароль спросит apksigner). Debug-ключом
#      релиз не подписывается.
#   4. В dist/<тег>/ — файлы релиза: peerage-<версия>-arm64-v8a.apk, update.json (манифест
#      обновления) и SHA256SUMS. Их и прикладывают к релизу на GitHub. Рядом — notes.md,
#      описание релиза из журнала приложения (assets/changelog/en.json на теге).
#
# Нужны: Docker (на Windows — в WSL, через scripts/w), Android SDK build-tools, ключ из
# scripts/local.env (PEERAGE_KEYSTORE).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f scripts/local.env ] && . scripts/local.env
. scripts/lib/sign.sh

# Где живут релизы: из этого строится ссылка на заметки в update.json.
REPO="peerage-chat/peerage"
# Раздаётся только сборка для телефонов; x86_64 — для эмулятора, в релиз не идёт.
ABI=arm64-v8a

die() { echo "release: $*" >&2; exit 1; }

# M.m.p → M·10000 + m·100 + p — то же правило, что в android/app/build.gradle.kts.
version_code() {
    local M m p
    IFS=. read -r M m p <<<"$1"
    echo $((M * 10000 + m * 100 + p))
}

# Версия из build.gradle.kts на коммите (не из рабочей копии: собирается тег).
version_at() {
    git show "$1:android/app/build.gradle.kts" | sed -n 's/^val APP_VERSION = "\(.*\)"$/\1/p'
}

TAG="${1:-}"
[ -n "$TAG" ] || die "укажи тег, например: scripts/release.sh v0.1.0"
commit="$(git rev-parse --verify -q "$TAG^{commit}")" || die "нет тега $TAG (git tag $TAG && git push origin $TAG)"
ver="$(version_at "$commit")"
[[ "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "на $TAG не прочитана версия (APP_VERSION в build.gradle.kts)"
[ "v$ver" = "$TAG" ] || die "тег $TAG, а в build.gradle.kts на нём версия $ver"
code="$(version_code "$ver")"

# Прошлый релиз — старший другой тег той же линии vM.m.* (0.4.2 после 0.5.0 сравнивается
# с 0.4.1); первый релиз линии — со старшим тегом вообще. Его versionCode должен быть меньше.
line="${TAG%.*}."
prev="$(git tag --list "${line}*" | grep -vx "$TAG" | sort -V | tail -1 || true)"
[ -n "$prev" ] || prev="$(git tag --list 'v*' | grep -vx "$TAG" | sort -V | tail -1 || true)"
if [ -n "$prev" ]; then
    pver="$(version_at "$prev")"
    [ -n "$pver" ] && [ "$(version_code "$pver")" -ge "$code" ] &&
        die "прошлый релиз $prev ($pver) не младше $TAG: обновление поверх не встанет"
fi

# Журнал приложения на теге: без записи этой версии «Что нового» молчит — проверить до сборки.
PY="$(command -v python3 || command -v python)" || die "нужен python3"
CHANGELOG=android/app/src/main/assets/changelog
for lang in en ru; do
    git show "$commit:$CHANGELOG/$lang.json" | "$PY" scripts/changelog-notes.py "$ver" >/dev/null ||
        die "в $CHANGELOG/$lang.json на $TAG нет записи $ver"
done

OUT="dist/$TAG"
BUILD="$OUT/build"
APK="peerage-$ver-$ABI.apk"
mkdir -p "$OUT"

echo "== 1/3 воспроизводимая сборка $TAG ($commit) =="
case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) scripts/w bash scripts/repro-build.sh "$BUILD" "$TAG" ;;
*) bash scripts/repro-build.sh "$BUILD" "$TAG" ;;
esac
unsigned="$BUILD/app-$ABI-release-unsigned.apk"
[ -f "$unsigned" ] || die "сборка не дала $unsigned"
[ "$(tr -d '\r\n' <"$BUILD/COMMIT")" = "$commit" ] || die "собран не тот коммит ($(cat "$BUILD/COMMIT"))"

echo "== 2/3 подпись =="
sign_apk "$unsigned" "$OUT/$APK" release

echo "== 3/3 манифест =="
sha="$(sha256sum "$OUT/$APK" | cut -d' ' -f1)"
size="$(wc -c <"$OUT/$APK" | tr -d ' ')"
date="$(git log -1 --format=%cs "$commit")"
cat >"$OUT/update.json" <<EOF
{
  "format": 1,
  "version_name": "$ver",
  "version_code": $code,
  "tag": "$TAG",
  "commit": "$commit",
  "date": "$date",
  "cert_sha256": "$SIGN_CERT_SHA256",
  "apks": [
    {"abi": "$ABI", "file": "$APK", "size": $size, "sha256": "$sha"}
  ],
  "notes": "https://github.com/$REPO/releases/tag/$TAG"
}
EOF
(cd "$OUT" && sha256sum "$APK" update.json >SHA256SUMS)
{
    git show "$commit:$CHANGELOG/en.json" | "$PY" scripts/changelog-notes.py "$ver"
    cat <<EOF

### Install

Download \`$APK\` and open it on the phone; it installs over earlier versions (same key).
The app also checks for updates itself once a day if allowed.

Signing certificate (SHA-256): \`$SIGN_CERT_SHA256\`
EOF
} >"$OUT/notes.md"

cat <<EOF

Готово: $OUT/
  $APK
  update.json
  SHA256SUMS
  notes.md — описание релиза
Отпечаток сертификата (SHA-256): $SIGN_CERT_SHA256

Дальше: релиз $TAG на GitHub (pre-release) с этими тремя файлами; описание — notes.md.
EOF
