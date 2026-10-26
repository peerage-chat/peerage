#!/usr/bin/env bash
# Внутри контейнера: собрать неподписанный APK из КОММИТА (не из рабочей копии).
#   /src — репозиторий только на чтение, /out — куда положить результат.
#   REF  — что собирать (по умолчанию HEAD), ABI — arm64-v8a.
#
# Сборка идёт из git clone: незакоммиченные правки, локальные .so и scripts/local.env
# в неё не попадают — иначе «воспроизводимо» значило бы «как у меня на машине».
set -euo pipefail
REF="${REF:-HEAD}"
ABI="${ABI:-arm64-v8a}"
W=/build/src

git config --global --add safe.directory '*'
git clone -q --no-hardlinks /src "$W"
cd "$W"
git checkout -q "$REF"
echo "== коммит $(git rev-parse HEAD) =="

# Время сборки = время коммита: всё, что всё же кладёт метку времени (zip, gradle),
# берёт её отсюда, а не из часов сборщика.
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
export TZ=UTC LC_ALL=C.UTF-8
export REPRO=1
export CARGO_TARGET_DIR=/build/target
# Пути в отладочной информации — к постоянным именам.
export RUSTFLAGS="--remap-path-prefix=$W=/src --remap-path-prefix=$CARGO_HOME=/cargo --remap-path-prefix=$RUSTUP_HOME=/rustup"
export CFLAGS="-ffile-prefix-map=$W=/src -ffile-prefix-map=$CARGO_HOME=/cargo"
export CXXFLAGS="$CFLAGS"

bash scripts/build-android-libs.sh "$ABI"

cd android
# Gradle без демона и без кэша между сборками: каждая — с нуля.
bash ./gradlew --no-daemon --no-build-cache -q assembleRelease
cd ..

mkdir -p /out
cp "android/app/build/outputs/apk/release/app-$ABI-release-unsigned.apk" /out/
cp android/app/src/main/jniLibs/"$ABI"/*.so /out/
git rev-parse HEAD > /out/COMMIT
(cd /out && sha256sum ./*.apk ./*.so | sort -k2 > SHA256SUMS && cat SHA256SUMS)
