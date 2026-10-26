#!/bin/bash
export ANDROID_NDK_HOME=~/android-ndk-r28c
export PATH="$PATH:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
cd "$(dirname "$0")/.."
cargo check -p peerage-android --target aarch64-linux-android 2>&1 | grep -E '^error|Checking peerage-android|Finished'
