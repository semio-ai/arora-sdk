extern crate cbindgen;

use std::env;
use std::path::PathBuf;

use cbindgen::Language;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    // The C header goes under OUT_DIR, and its directory to the build scripts
    // of the crates depending on this one, as DEP_ARORA_UTIL_INCLUDE (the
    // `links` key in Cargo.toml).
    let include_dir = PathBuf::from(env::var("OUT_DIR").unwrap()).join("include");

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(Language::C)
        .with_include_guard("_ARORA_UTIL_H_")
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(include_dir.join("arora").join("util.h"));
    println!("cargo:include={}", include_dir.display());
}
