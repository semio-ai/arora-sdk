//! Write the module directory a device loads `test-rust-wasm` from:
//! `header.json` and `records/`, both from the module's declaration, and,
//! when its path is given, the guest as `test_rust_wasm.wasm`. `records/` is
//! written afresh.
//!
//! ```text
//! cargo build -p test-rust-wasm --target wasm32-wasip2
//! cargo run -p test-rust-wasm --example module_dir -- \
//!     <dir> target/wasm32-wasip2/debug/test_rust_wasm.wasm
//! ```

use std::error::Error;
use std::path::PathBuf;

use arora_registry::local::LocalRegistry;
use arora_registry::local_yaml::write_records_to_yaml_dir;
use arora_types::module::low::Executor;
use test_rust_wasm::test_rust_wasm;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let dir = PathBuf::from(
        args.next()
            .ok_or("usage: module_dir <dir> [<guest .wasm>]")?,
    );
    let guest = args.next().map(PathBuf::from);

    let header = test_rust_wasm::header(Executor {
        name: "wasm-component".to_string(),
        min_version: None,
        max_version: None,
    });
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("header.json"),
        serde_json::to_string_pretty(&header)?,
    )?;

    // The module's record, at its header's version, and the records of the
    // types its functions name, with their scopes' folders.
    let mut registry = LocalRegistry::new();
    futures::executor::block_on(
        registry.add_declared_module::<test_rust_wasm::Module>(header.version.into()),
    )?;
    let records = dir.join("records");
    if records.exists() {
        std::fs::remove_dir_all(&records)?;
    }
    write_records_to_yaml_dir(&registry, &records)?;

    if let Some(guest) = guest {
        std::fs::copy(guest, dir.join("test_rust_wasm.wasm"))?;
    }
    Ok(())
}
