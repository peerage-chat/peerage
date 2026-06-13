use std::path::PathBuf;

fn main() {
    // libygg.so лежит в ../ygg/libygg относительно этого крейта.
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let libdir = manifest.parent().unwrap().join("ygg").join("libygg");

    println!("cargo:rustc-link-search=native={}", libdir.display());
    println!("cargo:rustc-link-lib=dylib=ygg");
    // rpath, чтобы бинарь находил libygg.so в рантайме без LD_LIBRARY_PATH.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libdir.display());
    println!(
        "cargo:rerun-if-changed={}",
        libdir.join("libygg.so").display()
    );
}
