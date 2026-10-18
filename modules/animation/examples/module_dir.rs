//! Write the module directory a device loads the guest from: `header.json`,
//! `records/` and, when its path is given, the guest
//! ([`arora_animation_module::module_dir`]).
//!
//! ```text
//! cargo build -p arora-animation-module --target wasm32-wasip1 --release
//! cargo run -p arora-animation-module --features module-dir --example module_dir -- \
//!     <dir> target/wasm32-wasip1/release/arora_animation_module.wasm
//! ```

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: module_dir <dir> [<guest .wasm>]"),
    );
    let guest = args.next().map(PathBuf::from);
    arora_animation_module::module_dir::write(&dir, guest.as_deref())
        .expect("the module directory is written");
}
