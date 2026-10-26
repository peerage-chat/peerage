# Android client

A Jetpack Compose app on top of the Rust engine. The UI is Kotlin; all domain logic
(identity, crypto, messaging, MLS groups, blobs, rooms) lives in the `peerage-engine` crate and
is reached through the thin JNI bridge `androidjni` (cdylib `libpeerage_android.so`).

```
Kotlin/Compose  ──JNI──▶  libpeerage_android.so (androidjni → peerage-engine)
                              │ depends on
                              ▼
                          libygg.so (Go c-shared, Yggdrasil)
```

The UI never calls `Native.*` on the main thread; see
[ARCHITECTURE.md §9](../docs/ARCHITECTURE.md#9-android-client-kotlincompose). What the app
can do and how it was verified: [project-state.md](../docs/project-state.md).

## Building the native libraries

Each ABI needs **two** libraries: `libygg.so` (Go) and `libpeerage_android.so` (Rust). They are
built on Linux or WSL (Go's cgo needs the Linux NDK).

### One-time toolchain setup (Linux / WSL)

```bash
# 1. The Linux NDK (the Windows NDK from Android Studio does not work for Go cgo):
cd ~ && wget https://dl.google.com/android/repository/android-ndk-r28c-linux.zip
unzip -q android-ndk-r28c-linux.zip
export ANDROID_NDK_HOME=$HOME/android-ndk-r28c   # add to ~/.bashrc

# 2. Rust targets + cargo-ndk:
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

### Build

```bash
bash scripts/build-android-libs.sh arm64-v8a   # phones; x86_64 for the emulator
bash scripts/rebuild-peerage-android.sh x86_64     # faster: only the Rust library
```

The scripts put the libraries into `android/app/src/main/jniLibs/<ABI>/` (not in git).
Build them BEFORE Gradle.

## Building the APK

```bash
cd android && ./gradlew assembleDebug
```

Or open `android/` in Android Studio. Run `./gradlew ktlintFormat` before committing Kotlin,
and `./gradlew testDebugUnitTest` for the unit tests.

## Toolchain versions

AGP 8.5.2 · Kotlin 1.9.25 · Compose compiler 1.5.15 · compileSdk 34 · minSdk 24 · JDK 21
(the one bundled with Android Studio).
