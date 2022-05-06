use std::path::PathBuf;

use cmake;

fn main() {
    // Builds the project in the directory located in `libfoo`, installing it
    // into $OUT_DIR
    // let dst = cmake::Config::new(".")
    //     .define("CMAKE_TOOLCHAIN_FILE", "i686.cmake")
    //     .build();
    // println!("cargo:rustc-link-search=native={}", dst.display());
    // println!("cargo:rustc-link-lib=static=dummy_cpp");

    let dst = configure();
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rustc-link-search=native={}", dst.display());
}

fn configure() -> PathBuf {
    cmake::Config::new(".")
        .define("CMAKE_TOOLCHAIN_FILE", "i686.cmake")
        .build()
}
