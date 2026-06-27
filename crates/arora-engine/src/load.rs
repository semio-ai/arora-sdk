//! Module-loading convenience helpers.
//!
//! `Engine::load_module` itself takes a [`ModuleDefinition`] (header +
//! executable bytes). Callers typically also need to remember which module
//! a function id belongs to so that subsequent `arora_call`s can be
//! dispatched without the caller wiring up that map themselves.
//!
//! These helpers package both concerns. They are pure (no I/O, no
//! registry), so they are usable identically from the native CLI and
//! from a browser-hosted `Engine`.

use uuid::Uuid;

use crate::engine::{Engine, LoadModuleError};
use crate::schema::module::low::{Header, ModuleDefinition};

/// What was loaded, in a form convenient for building a
/// function-id → module-id index.
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub id: Uuid,
    pub function_ids: Vec<Uuid>,
}

/// Build a [`ModuleDefinition`] from its constituent parts.
pub fn module_definition_from_parts(header: Header, executable: Box<[u8]>) -> ModuleDefinition {
    ModuleDefinition {
        schema_version: 0,
        header,
        executable,
    }
}

/// Load a module into `engine` from its `header` and `executable` bytes,
/// returning a `LoadedModule` summary the caller can use to map function
/// ids back to the module id.
pub fn load_module_from_parts(
    engine: &mut Engine,
    header: Header,
    executable: Box<[u8]>,
) -> Result<LoadedModule, LoadModuleError> {
    let id = header.id;
    let function_ids = header.exports.iter().map(|e| *e.id()).collect();
    engine.load_module(module_definition_from_parts(header, executable))?;
    Ok(LoadedModule { id, function_ids })
}
