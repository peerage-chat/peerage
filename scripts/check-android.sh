#!/bin/bash
export ANDROID_NDK_HOME=~/android-ndk-r28c
export PATH="$PATH:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
cd /mnt/c/Users/fixman/Documents/projects/p2p
cargo check -p p2p-android --target aarch64-linux-android 2>&1 | grep -E '^error|Checking p2p-android|Finished'
