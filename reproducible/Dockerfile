# Воспроизводимая сборка Android-клиента (план беты, шаг 7).
#
# Всё, от чего зависит результат, закреплено здесь: базовый образ — по digest,
# пакеты Debian — снимком репозитория на дату, тулчейны — версией И sha256 архива.
# Хэши сверены с официальными источниками (go.dev JSON, rustup .sha256, манифест
# Google repository2-3.xml, gradle .sha256) 2026-10-26.
#
# Сборка образа и APK: scripts/repro-build.sh (см. reproducible/README.md).
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

# Пакеты — со снимка snapshot.debian.org: через год «apt install cmake» даст ту же
# версию, что сегодня. Check-Valid-Until выключен: подпись Release у снимка старая
# по определению, но сама подпись проверяется.
ARG DEBIAN_SNAPSHOT=20261020T000000Z
RUN rm -f /etc/apt/sources.list.d/debian.sources \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} bookworm main" > /etc/apt/sources.list \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT} bookworm-security main" >> /etc/apt/sources.list \
 && apt-get -o Acquire::Retries=5 update \
 && DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y --no-install-recommends \
      ca-certificates curl unzip git python3 perl make cmake ninja-build pkg-config \
      build-essential openjdk-17-jdk-headless \
 && rm -rf /var/lib/apt/lists/*

ENV DL="curl -fsSL --retry 8 --retry-all-errors --retry-delay 3"

# Go — та же версия, что у разработчика (go.mod требует ≥1.24).
ARG GO_VERSION=1.26.4
ARG GO_SHA256=1153d3d50e0ac764b447adfe05c2bcf08e889d42a02e0fe0259bd47f6733ad7f
RUN $DL -o /tmp/go.tgz https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz \
 && echo "${GO_SHA256}  /tmp/go.tgz" | sha256sum -c - \
 && tar -C /usr/local -xzf /tmp/go.tgz && rm /tmp/go.tgz
ENV PATH=/usr/local/go/bin:$PATH GOTOOLCHAIN=local

# Rust — rustup фиксированной версии, тулчейн фиксированной версии.
ARG RUSTUP_SHA256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
ARG RUST_VERSION=1.96.0
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN $DL -o /tmp/rustup-init https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init \
 && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION} \
      --target aarch64-linux-android --target x86_64-linux-android \
 && rm /tmp/rustup-init \
 && cargo install --locked cargo-ndk@4.1.2

# NDK — тот же r28c, что у разработчика.
ARG NDK_SHA256=dfb20d396df28ca02a8c708314b814a4d961dc9074f9a161932746f815aa552f
RUN $DL -o /tmp/ndk.zip https://dl.google.com/android/repository/android-ndk-r28c-linux.zip \
 && echo "${NDK_SHA256}  /tmp/ndk.zip" | sha256sum -c - \
 && unzip -q /tmp/ndk.zip -d /opt && rm /tmp/ndk.zip
ENV ANDROID_NDK_HOME=/opt/android-ndk-r28c

# Android SDK: cmdline-tools по хэшу, платформа и build-tools — через sdkmanager
# (он сам сверяет sha1 из манифеста Google). Лицензия — хэш, уже принятый владельцем
# проекта в его локальном SDK; принимать соглашения в сборке молча мы не хотим.
ARG CLT_SHA256=7ec965280a073311c339e571cd5de778b9975026cfcbe79f2b1cdcb1e15317ee
ENV ANDROID_HOME=/opt/android-sdk
COPY android-sdk-licenses/ /opt/android-sdk/licenses/
RUN $DL -o /tmp/clt.zip https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip \
 && echo "${CLT_SHA256}  /tmp/clt.zip" | sha256sum -c - \
 && mkdir -p $ANDROID_HOME/cmdline-tools && unzip -q /tmp/clt.zip -d $ANDROID_HOME/cmdline-tools \
 && mv $ANDROID_HOME/cmdline-tools/cmdline-tools $ANDROID_HOME/cmdline-tools/latest && rm /tmp/clt.zip \
 # platform-tools Gradle иначе докачивает сам ВО ВРЕМЯ сборки. Версию sdkmanager
 # закрепить не даёт (только последняя), но в APK из него ничего не попадает.
 && $ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager --install "platforms;android-34" "build-tools;34.0.0" "platform-tools" \
 && rm -rf /root/.android

# Постоянный путь сборки: пути попадают в отладочную информацию, и разные каталоги
# у двух сборщиков дали бы разные байты.
WORKDIR /build
COPY build-inside.sh /usr/local/bin/build-inside.sh
ENTRYPOINT ["bash", "/usr/local/bin/build-inside.sh"]
