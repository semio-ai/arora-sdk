// libpiper's directory on the rpath of every binary of this package, when
// the `piper` feature links it: the say-piper crate's link arguments cover its
// own artifacts only, and publishes the directory as DEP_PIPER_INSTALL_DIR.
fn main() {
    println!("cargo:rerun-if-env-changed=DEP_PIPER_INSTALL_DIR");
    if let Ok(dir) = std::env::var("DEP_PIPER_INSTALL_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}/lib");
    }
}
