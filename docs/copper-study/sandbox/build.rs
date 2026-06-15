// Copper's #[copper_runtime] proc macro records its compile-time string-interning
// log index under LOG_INDEX_DIR. Every Copper app/crate must set this in build.rs.
fn main() {
    println!("cargo:rustc-env=LOG_INDEX_DIR={}", std::env::var("OUT_DIR").unwrap());
}
