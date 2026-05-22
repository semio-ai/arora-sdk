use std::env;

fn main() {
    let arora_cli = env::var("CARGO_BIN_FILE_ARORA_CLI")
        .expect("CARGO_BIN_FILE_ARORA_CLI not set; bindeps may not be enabled (-Z bindeps)");
    println!("cargo:rustc-env=ARORA_CLI_BIN={arora_cli}");
    println!("cargo:rerun-if-changed=build.rs");
}
