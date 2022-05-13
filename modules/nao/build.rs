use std::{env, path::PathBuf};

use cmake;

fn main() {
    if env::var("ENABLE_NAO").unwrap_or("0".to_string()) == "1" {
        let dst = configure();
        println!("cargo:rerun-if-changed=CMakeLists.txt");
        println!("cargo:rustc-link-search=native={}", dst.display());
    }
}

fn configure() -> PathBuf {
    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let binary_dir = out_dir
        .parent() // crate build script build dir
        .unwrap()
        .parent() // "build" dir
        .unwrap()
        .parent() // build type dir (release / debug)
        .unwrap();

    let source_dir = binary_dir
        .parent() // target dir (i686-...)
        .unwrap()
        .parent()
        .unwrap() // "target" dir
        .parent()
        .unwrap();

    let arora_behavior_tree_dir = source_dir
        .join("crates")
        .join("arora-behavior-tree-types-yaml")
        .join("records");

    let arora_module_cli_path = binary_dir
        .parent() // target dir (i686-...)
        .unwrap()
        .parent()
        .unwrap() // "target" dir
        .join("debug")
        .join("arora-module-cli");

    let arora_cpp_source_dir = source_dir.join("libs").join("cpp");
    
    cmake::Config::new(".")
        .define("CMAKE_TOOLCHAIN_FILE", "mac-homebrew-i686.toolchain.cmake")
        .define("ARORA_BINARY_DIR", binary_dir)
        .define("ARORA_MODULE_CLI", arora_module_cli_path)
        .define("ARORA_BEHAVIOR_TREE_INCLUDE", arora_behavior_tree_dir)
        .define("ARORA_CPP_SOURCE_DIR", arora_cpp_source_dir)
        .build()
}
