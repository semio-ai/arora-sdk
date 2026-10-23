//! Browser-hosted Arora executor.
//!
//! Instantiates guest components (`executor.name: wasm-component`) through
//! the browser's native `WebAssembly` runtime (no wasmtime):
//! [`BrowserComponentExecutor`] loads them, and a [`SharedLoader`] stages
//! those too large to compile synchronously.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use uuid::Uuid;
use wasm_bindgen::prelude::*;

use super::LoadModuleError;

mod component;

use crate::engine::EngineRef;

pub use component::BrowserComponentExecutor;
pub(crate) use component::{compile as compile_component, BrowserComponent};

/// State shared between the [`BrowserComponentExecutor`]s of an engine and
/// asynchronous module loaders (e.g. arora-web's `prepareModule`).
///
/// Chrome disallows both synchronous compilation and synchronous
/// instantiation above 8 MB on the main thread, so large modules must be
/// compiled *and* instantiated through the async `WebAssembly.instantiate`.
/// [`SharedLoader::prepare`] does that and stages the ready instance here;
/// the next `Executor::load_module` for the same module id picks it up
/// instead of compiling synchronously.
pub struct SharedLoader {
    engine: Cell<Option<EngineRef>>,
    prepared: RefCell<HashMap<Uuid, component::PreparedComponent>>,
}

pub type SharedLoaderRc = Rc<SharedLoader>;

impl SharedLoader {
    /// A loader staging nothing yet, its engine set by the first executor
    /// built on it to join an engine.
    pub fn new() -> SharedLoaderRc {
        Rc::new(SharedLoader {
            engine: Cell::new(None),
            prepared: RefCell::new(HashMap::new()),
        })
    }

    /// Asynchronously compile and instantiate the component `executable` for
    /// the module `id`, staging the instance for the next `load_module(id)`
    /// of a [`BrowserComponentExecutor`] built on this loader. A core module,
    /// which no executor loads, is refused.
    pub async fn prepare(self: Rc<Self>, id: Uuid, executable: Vec<u8>) -> Result<(), JsValue> {
        let engine_ptr = self
            .engine
            .get()
            .ok_or_else(|| JsValue::from_str("SharedLoader: set_engine not called"))?;
        if !crate::component_layout::is_component(&executable) {
            return Err(JsValue::from_str(&format!(
                "module {id} is a core module: Arora loads guests as components, built for \
                 wasm32-wasip2"
            )));
        }
        let prepared = component::prepare(engine_ptr, &executable).await?;
        self.prepared.borrow_mut().insert(id, prepared);
        Ok(())
    }
}

fn js_to_load_err(e: JsValue) -> LoadModuleError {
    LoadModuleError::Internal(format!("js error: {e:?}"))
}
