//! Component-model executor.
//!
//! Hosts guest modules built as WebAssembly Components (e.g. Rust's
//! `wasm32-wasip2` target) against the `arora:module` WIT world defined in
//! `crates/arora-engine/wit/arora-module.wit`. Unlike the core-module executor in
//! [`super::wasm`], data crosses the boundary through the canonical ABI
//! (`list<u8>`), so there is no guest allocator to drive and no raw linear
//! memory to read.
//!
//! Selected by `executor.name: wasm-component` in a module's header.

use anyhow::Result;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Config, Engine as WasmEngine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use arora_types::module::low::ModuleDefinition;

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::engine::EngineRef;
use crate::module::{DispatchError, Module as AroraModule};

bindgen!({
    world: "module",
    path: "wit",
});

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
    Internal(#[error(not(source))] anyhow::Error),
}

/// [`EngineRef`] wrapper satisfying the `Send` bound wasmtime puts on store
/// data.
///
/// SAFETY: the engine and the modules holding this pointer live and run on a
/// single thread; `Send` is only claimed to satisfy the bound, the pointer
/// never actually crosses threads.
struct EnginePtr(EngineRef);
unsafe impl Send for EnginePtr {}

/// Host-side state held by each component's [`Store`].
struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,

    /// The engine this module belongs to. Guest calls to `host.dispatch`
    /// re-enter the engine while it is already mutably borrowed further up
    /// the stack, hence the raw pointer (see [`EngineRef`]).
    engine: EnginePtr,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn uuid_to_id(uuid: &Uuid) -> Id {
    let (hi, lo) = uuid.as_u64_pair();
    Id { hi, lo }
}

fn id_to_uuid(id: Id) -> Uuid {
    Uuid::from_u64_pair(id.hi, id.lo)
}

impl arora::module::types::Host for HostState {}

impl arora::module::host::Host for HostState {
    fn dispatch(&mut self, module: Id, method: Id, arg: Vec<u8>) -> Result<Vec<u8>, String> {
        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let engine = unsafe { &mut *self.engine.0 };
        engine
            .dispatch(&id_to_uuid(module), &id_to_uuid(method), &arg)
            .map(Vec::from)
            .map_err(|e| format!("{e}"))
    }

    fn dispatch_indirect(&mut self, callable: u64) -> Result<Vec<u8>, String> {
        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let bridge: &mut dyn CallBridge = unsafe { &mut *self.engine.0 };
        let value = bridge
            .arora_call_indirect(&CallableId { id: callable })
            .map_err(|e| format!("{e}"))?;
        Ok(arora_buffers::serde_uuid::serialize(&value).into())
    }
}

pub struct ComponentExecutor {
    engine: WasmEngine,
    linker: Linker<HostState>,
    arora_engine: Option<EngineRef>,
}

impl ComponentExecutor {
    pub fn new() -> Result<Self, InitializationError> {
        let mut config = Config::new();
        config.debug_info(cfg!(debug_assertions));
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);

        let engine = WasmEngine::new(&config).map_err(anyhow::Error::from)?;

        let mut linker: Linker<HostState> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(anyhow::Error::from)?;
        Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)
            .map_err(anyhow::Error::from)?;

        Ok(Self {
            engine,
            linker,
            arora_engine: None,
        })
    }
}

impl Executor for ComponentExecutor {
    fn set_engine(&mut self, engine: EngineRef) {
        self.arora_engine = Some(engine);
    }

    fn name(&self) -> &'static str {
        "wasm-component"
    }

    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        let component = Component::new(&self.engine, &module_definition.executable)
            .map_err(|_| LoadModuleError::MalformedExecutable)?;

        let state = HostState {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::new(),
            engine: EnginePtr(self.arora_engine.ok_or_else(|| {
                LoadModuleError::Internal("ComponentExecutor: set_engine not called".into())
            })?),
        };
        let mut store = Store::new(&self.engine, state);

        let bindings = Module::instantiate(&mut store, &component, &self.linker)
            .map_err(|e| LoadModuleError::Internal(format!("instantiate: {e}")))?;

        let function_ids = module_definition
            .header
            .exports
            .iter()
            .map(|e| *e.id())
            .collect();

        Ok(Box::new(ComponentModule {
            bindings,
            store,
            function_ids,
        }))
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), UnloadModuleError> {
        // The instance is dropped along with the ComponentModule held by
        // the engine; nothing else to do.
        Ok(())
    }
}

struct ComponentModule {
    bindings: Module,
    store: Store<HostState>,
    /// Functions the module's header declares; dispatch refuses others.
    function_ids: Vec<Uuid>,
}

impl AroraModule for ComponentModule {
    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        if !self.function_ids.contains(method_id) {
            return Err(DispatchError::Internal {
                message: format!("function {method_id} not exported by this module"),
            });
        }
        let result = self
            .bindings
            .call_dispatch(&mut self.store, uuid_to_id(method_id), arg)
            .map_err(|e| DispatchError::Trap {
                message: format!("dispatch trapped: {e:#?}"),
            })?;
        match result {
            Ok(bytes) => Ok(bytes.into_boxed_slice()),
            Err(message) => Err(DispatchError::Guest { message }),
        }
    }
}
