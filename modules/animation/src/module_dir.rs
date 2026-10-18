//! The module directory a device loads the guest from (`module-dir`):
//!
//! ```text
//! <dir>/
//! ├── header.json            the declared header, for the wasm executor
//! ├── arora-animation.wasm   the guest
//! └── records/               the module's record and the records of the
//!                            types its functions name, in arora-registry's
//!                            layout
//! ```
//!
//! A device given the directory's records (the `arora` runner loads them
//! from `records/`; arora-web takes them with `withRecords`) describes every
//! export from the module's record, the record-typed ones a behavior calls
//! — `step_values`, `player_states` — included. The records live in the
//! `arora` scope.

use std::io;
use std::path::Path;

use arora_registry::local::LocalRegistry;
use arora_registry::local_yaml::write_records_to_yaml_dir;
use arora_types::module::low::{Executor, Header};

use crate::animation;

/// The guest's file name in a module directory.
pub const GUEST_FILE: &str = "arora-animation.wasm";

/// The header the guest is loaded with, for the wasm executor.
pub fn guest_header() -> Header {
    animation::header(Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    })
}

/// A registry holding the module's record, at its header's version, and the
/// records of the types its functions name, with their scope's folder.
pub fn records() -> LocalRegistry {
    let mut registry = LocalRegistry::new();
    futures::executor::block_on(
        registry.add_declared_module::<animation::Module>(guest_header().version.into()),
    )
    .expect("the declaration's records add to an empty registry");
    registry
}

/// Write the module directory `dir`: its `header.json`, its `records/`, and,
/// when `guest` is given, the guest compiled for `wasm32-wasip1` as
/// [`GUEST_FILE`]. `records/` is written afresh.
pub fn write(dir: &Path, guest: Option<&Path>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let header = serde_json::to_string_pretty(&guest_header()).map_err(io::Error::other)?;
    std::fs::write(dir.join("header.json"), header)?;
    let records = dir.join("records");
    if records.exists() {
        std::fs::remove_dir_all(&records)?;
    }
    write_records_to_yaml_dir(&self::records(), &records).map_err(io::Error::other)?;
    if let Some(guest) = guest {
        std::fs::copy(guest, dir.join(GUEST_FILE))?;
    }
    Ok(())
}
