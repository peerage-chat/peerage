use std::path::PathBuf;

fn main() {
    // libygg.so (c-shared обёртка вокруг yggdrasil-go). На десктопе лежит в
    // ../ygg/libygg; под Android — в ../ygg/libygg/android/<ABI> (по одной на ABI).
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let base = manifest.parent().unwrap().join("ygg").join("libygg");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_android = target_os == "android";

    let libdir = if is_android {
        // arch → Android ABI (имя подпапки и одновременно каталога jniLibs).
        let abi = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default().as_str() {
            "aarch64" => "arm64-v8a",
            "arm" => "armeabi-v7a",
            "x86_64" => "x86_64",
            "x86" => "x86",
            other => Box::leak(other.to_string().into_boxed_str()),
        };
        base.join("android").join(abi)
    } else {
        base
    };

    println!("cargo:rustc-link-search=native={}", libdir.display());
    println!("cargo:rustc-link-lib=dylib=ygg");

    // rpath нужен только десктопу (бинарь ищет .so рядом). На Android .so
    // кладётся в jniLibs и грузится системным линкером — rpath не применим.
    if !is_android {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libdir.display());
    }

    println!("cargo:rerun-if-changed={}", libdir.join("libygg.so").display());
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}
