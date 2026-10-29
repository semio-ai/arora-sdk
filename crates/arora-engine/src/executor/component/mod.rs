//! Component-model executor.
//!
//! Hosts guest modules built as WebAssembly Components (e.g. Rust's
//! `wasm32-wasip2` target) against the `arora:module` WIT world defined in
//! `crates/arora-engine/wit/arora-module.wit`. Data crosses the boundary
//! through the canonical ABI (`list<u8>`), so there is no guest allocator to
//! drive and no raw linear memory to read.
//!
//! Selected by `executor.name: wasm-component` in a module's header.
//!
//! A component may import the WASI 0.2 interfaces an ordinary program's
//! runtime reaches for — `wasi:cli` (environment, exit, standard streams,
//! terminals), `wasi:io`, `wasi:clocks`, `wasi:random` and `wasi:filesystem`,
//! with no directory preopened, so a guest sees no file — and no other: no
//! sockets ([`HOST_INTERFACES`](crate::component_layout::HOST_INTERFACES)).
//! A C or C++ guest's libc reaches the filesystem's interfaces whenever it
//! links its file descriptors' code, as a debug build of the Rust libraries it
//! links does. A component importing another interface fails to instantiate.
//!
//! A component instance a call trapped in — a Rust guest's panic, a C++
//! guest's failed `assert`, a call interrupted at its deadline
//! ([`crate::limits`]) — is entered no more: wasmtime refuses to, and the
//! trap may have left its state half written. The call fails with its trap;
//! before the module's next call, the executor instantiates the component
//! afresh, from its compiled code, and that call runs in the new instance,
//! from the module's initial state. The executor logs a warning naming the
//! module and the trap. Instantiating afresh runs, as loading does, under
//! [`DEFAULT_CALL_BUDGET`](limits::DEFAULT_CALL_BUDGET); a module that fails
//! to instantiate afresh runs no more calls until it is reloaded.

use std::sync::OnceLock;

use anyhow::Result;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::component::{bindgen, Component, HasData, Linker};
use wasmtime::{Config, Engine as WasmEngine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use arora_types::module::low::{Header, ModuleDefinition};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::compiled::{Code, CompiledModule};
use crate::engine::EngineRef;
use crate::limits::{self, GuestLimits};
use crate::load::module_definition_from_parts;
use crate::module::{guest_error, DispatchError, Module as AroraModule};

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
    /// What the instance may grow its memory to ([`GuestLimits::memory`]).
    limits: StoreLimits,

    /// The engine this module belongs to. Guest calls to `host.dispatch`
    /// re-enter the engine while it is already mutably borrowed further up
    /// the stack, hence the raw pointer (see [`EngineRef`]).
    engine: EnginePtr,
    /// When the call under way must end ([`limits::bound`]): the
    /// instantiation's deadline until a call sets its own.
    deadline: std::time::Instant,
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
    linker: Linker<HostState>,
    arora_engine: Option<EngineRef>,
}

/// The configuration every component compiles and runs under.
///
/// The `arora:module` world declares no async function, stream or future, so
/// a component built against it never uses the component model's concurrency
/// ABI. Turning concurrency support off makes compilation refuse a component
/// that does: such a component is not an Arora module, and refusing it keeps
/// wasmtime's async lifting, lowering and task machinery out of a guest's
/// reach.
fn config() -> Config {
    let mut config = Config::new();
    config.debug_info(cfg!(debug_assertions));
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    config.concurrency_support(false);
    // A call's deadline (`crate::limits`) is checked against the engine's
    // epoch, which `limits::tick` advances.
    config.epoch_interruption(true);
    config
}

/// The wasmtime engine every component compiles on and every instance runs
/// in: one per process, created on first use. It allocates instances on
/// demand, reserving address space per live instance, where a pooling
/// allocator reserves it up front for every instance the pool can hold. A
/// `Component` instantiates only in a store of the engine that compiled
/// it, so one engine is what lets the devices of a process share a compiled
/// module; each instance keeps a store of its own. A failure to create it is
/// kept: every later executor and compilation fails with it.
fn engine() -> Result<&'static WasmEngine, LoadModuleError> {
    static ENGINE: OnceLock<Result<WasmEngine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let engine = WasmEngine::new(&config()).map_err(|e| format!("{e:#}"))?;
            limits::tick(&engine);
            Ok(engine)
        })
        .as_ref()
        .map_err(|e| {
            LoadModuleError::Internal(format!("failed to create the component engine: {e}"))
        })
}

/// Compile `executable` on the process-wide [`engine`].
pub(crate) fn compile(executable: &[u8]) -> Result<Component, LoadModuleError> {
    Component::new(engine()?, executable).map_err(|_| LoadModuleError::MalformedExecutable)
}

/// `wasi:io`'s host state: the store's resource table.
struct HasIo;

impl HasData for HasIo {
    type Data<'a> = &'a mut ResourceTable;
}

/// Link the WASI 0.2 interfaces a component may import (see the module
/// documentation), as `wasmtime_wasi::p2::add_to_linker_sync` links them, the
/// sockets left out. The filesystem holds no directory: the context preopens
/// none.
fn add_wasi_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    use wasmtime_wasi::cli::{WasiCli, WasiCliView};
    use wasmtime_wasi::clocks::{WasiClocks, WasiClocksView};
    use wasmtime_wasi::filesystem::{WasiFilesystem, WasiFilesystemView};
    use wasmtime_wasi::p2::bindings::{cli, clocks, filesystem, io, random, sync};
    use wasmtime_wasi::random::WasiRandom;

    let l = linker;
    io::error::add_to_linker::<_, HasIo>(l, |t| t.ctx().table)?;
    sync::io::poll::add_to_linker::<_, HasIo>(l, |t| t.ctx().table)?;
    sync::io::streams::add_to_linker::<_, HasIo>(l, |t| t.ctx().table)?;
    clocks::wall_clock::add_to_linker::<_, WasiClocks>(l, HostState::clocks)?;
    clocks::monotonic_clock::add_to_linker::<_, WasiClocks>(l, HostState::clocks)?;
    filesystem::preopens::add_to_linker::<_, WasiFilesystem>(l, HostState::filesystem)?;
    sync::filesystem::types::add_to_linker::<_, WasiFilesystem>(l, HostState::filesystem)?;
    random::random::add_to_linker::<_, WasiRandom>(l, |t| t.ctx().ctx.random())?;
    random::insecure::add_to_linker::<_, WasiRandom>(l, |t| t.ctx().ctx.random())?;
    random::insecure_seed::add_to_linker::<_, WasiRandom>(l, |t| t.ctx().ctx.random())?;
    cli::exit::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::environment::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::stdin::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::stdout::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::stderr::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::terminal_input::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::terminal_output::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::terminal_stdin::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::terminal_stdout::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    cli::terminal_stderr::add_to_linker::<_, WasiCli>(l, HostState::cli)?;
    Ok(())
}

impl ComponentExecutor {
    /// An executor on the process-wide component engine, which this creates if
    /// no executor has yet. Fails if the engine cannot be created.
    pub fn new() -> Result<Self, InitializationError> {
        let engine = engine().map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut linker: Linker<HostState> = Linker::new(engine);
        add_wasi_to_linker(&mut linker).map_err(anyhow::Error::from)?;
        Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)
            .map_err(anyhow::Error::from)?;

        Ok(Self {
            linker,
            arora_engine: None,
        })
    }

    /// Instantiate `component` in a store of its own, linked to this
    /// executor's Arora engine, serving the functions `header` declares.
    fn instantiate(
        &self,
        header: &Header,
        component: &Component,
        limits: GuestLimits,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        let engine = self.arora_engine.ok_or_else(|| {
            LoadModuleError::Internal("ComponentExecutor: set_engine not called".into())
        })?;
        let pre = self
            .linker
            .instantiate_pre(component)
            .and_then(ModulePre::new)
            .map_err(|e| LoadModuleError::Internal(format!("instantiate: {e:#}")))?;
        let function_ids: Vec<Uuid> = header.exports.iter().map(|e| *e.id()).collect();
        let instance = instance(&pre, engine, limits.memory, &function_ids)?;
        Ok(Box::new(ComponentModule {
            id: header.id,
            name: header.name.clone(),
            budget: limits::DEFAULT_CALL_BUDGET,
            pre,
            engine,
            memory: limits.memory,
            instance,
            function_ids,
            state: State::Ready,
            entered: 0,
        }))
    }
}

/// A component's instance: its store, and its exports bound in it.
struct Instance {
    bindings: Module,
    store: Store<HostState>,
}

/// An instance of `pre` in a store of its own, linked to the Arora engine
/// `engine`, each of its memories held to `memory` bytes, serving every
/// function of `function_ids`. Instantiating runs the component's initializer
/// and its `functions`, under [`DEFAULT_CALL_BUDGET`](limits::DEFAULT_CALL_BUDGET).
fn instance(
    pre: &ModulePre<HostState>,
    engine: EngineRef,
    memory: usize,
    function_ids: &[Uuid],
) -> Result<Instance, LoadModuleError> {
    let state = HostState {
        wasi: WasiCtxBuilder::new().inherit_stdio().build(),
        table: ResourceTable::new(),
        limits: StoreLimitsBuilder::new().memory_size(memory).build(),
        engine: EnginePtr(engine),
        deadline: std::time::Instant::now() + limits::DEFAULT_CALL_BUDGET,
    };
    let mut store = Store::new(pre.engine(), state);
    store.limiter(|state| &mut state.limits);
    limits::bound(&mut store, |state: &HostState| state.deadline);

    let bindings = pre
        .instantiate(&mut store)
        .map_err(|e| LoadModuleError::Internal(format!("instantiate: {e:#}")))?;

    // Every function the header declares is one the component serves.
    let served = bindings
        .call_functions(&mut store)
        .map_err(|e| LoadModuleError::Internal(format!("functions trapped: {e:#}")))?;
    if let Some(missing) = function_ids
        .iter()
        .find(|id| !served.iter().any(|served| id_to_uuid(*served) == **id))
    {
        return Err(LoadModuleError::Internal(format!(
            "failed to find function export {missing}: the component does not serve it"
        )));
    }
    Ok(Instance { bindings, store })
}

impl Executor for ComponentExecutor {
    fn set_engine(&mut self, engine: EngineRef) {
        self.arora_engine = Some(engine);
    }

    fn name(&self) -> &'static str {
        "wasm-component"
    }

    fn loads_compiled_modules(&self) -> bool {
        true
    }

    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        self.load_module_limited(module_definition, GuestLimits::default())
    }

    fn load_compiled_module(
        &mut self,
        module: &CompiledModule,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        self.load_compiled_module_limited(module, GuestLimits::default())
    }

    fn load_module_limited(
        &mut self,
        module_definition: ModuleDefinition,
        limits: GuestLimits,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        let component = compile(&module_definition.executable)?;
        self.instantiate(&module_definition.header, &component, limits)
    }

    fn load_compiled_module_limited(
        &mut self,
        module: &CompiledModule,
        limits: GuestLimits,
    ) -> Result<Box<dyn AroraModule>, LoadModuleError> {
        match &module.code {
            Code::Component(component) => self.instantiate(&module.header, component, limits),
            Code::Executable(executable) => self.load_module_limited(
                module_definition_from_parts(module.header.clone(), executable.as_ref().into()),
                limits,
            ),
        }
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), UnloadModuleError> {
        // The instance is dropped along with the ComponentModule held by
        // the engine; nothing else to do.
        Ok(())
    }
}

struct ComponentModule {
    /// The module's id and name, which an error names.
    id: Uuid,
    name: String,
    /// The budget of the call under way, which an overrun's error names.
    budget: std::time::Duration,
    /// The compiled component, linked, which instantiates it afresh after a
    /// trap, linked to `engine`, each memory held to `memory` bytes.
    pre: ModulePre<HostState>,
    engine: EngineRef,
    memory: usize,
    instance: Instance,
    /// Functions the module's header declares; dispatch refuses others.
    function_ids: Vec<Uuid>,
    state: State,
    /// How many calls into `instance` are under way: more than one while the
    /// guest is called back. An instance in use is not replaced: its guest
    /// still runs in it.
    entered: usize,
}

/// Whether a module's instance may be entered.
enum State {
    Ready,
    /// A call trapped in it, as the message says: the module instantiates
    /// afresh before its next call.
    Trapped(String),
    /// Instantiating afresh failed, as the message says: the module runs no
    /// more calls until it is reloaded.
    Lost(String),
}

impl ComponentModule {
    /// Make the module fit to run a call to `function`: one whose call
    /// trapped is instantiated afresh first, its call's deadline carried over.
    /// The call is refused while the instance that trapped is in use, once
    /// instantiating afresh has failed, and when no time is left before the
    /// call's deadline.
    fn recover(&mut self, function: &Uuid) -> Result<(), DispatchError> {
        let trap = match &self.state {
            State::Ready => return Ok(()),
            State::Lost(message) => {
                return Err(DispatchError::Trap {
                    message: message.clone(),
                })
            }
            State::Trapped(trap) => trap.clone(),
        };
        if self.entered > 0 {
            return Err(DispatchError::Trap {
                message: format!("{TRAPPED} ({trap})"),
            });
        }
        let deadline = self.instance.store.data().deadline;
        // A guest calling the module back while it instantiates finds it in
        // use.
        self.entered += 1;
        let fresh = instance(&self.pre, self.engine, self.memory, &self.function_ids);
        self.entered -= 1;
        match fresh {
            Ok(mut fresh) => {
                fresh.store.data_mut().deadline = deadline;
                self.instance = fresh;
                self.state = State::Ready;
                log::warn!(
                    "module '{}' was instantiated afresh after a trap: {trap}",
                    self.name
                );
            }
            Err(error) => {
                let message = format!("{LOST} (trap: {trap}; instantiating: {error})");
                log::warn!("module '{}': {message}", self.name);
                self.state = State::Lost(message.clone());
                return Err(DispatchError::Trap { message });
            }
        }
        // Instantiating may have taken the call's time: the new instance,
        // never entered, serves the next call.
        if deadline <= std::time::Instant::now() {
            return Err(DispatchError::Trap {
                message: limits::no_time_left(&self.id, function),
            });
        }
        Ok(())
    }
}

impl AroraModule for ComponentModule {
    fn set_call_deadline(&mut self, deadline: std::time::Instant, budget: std::time::Duration) {
        self.instance.store.data_mut().deadline = deadline;
        self.budget = budget;
    }

    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        if !self.function_ids.contains(method_id) {
            return Err(DispatchError::FunctionNotFound { id: *method_id });
        }
        self.recover(method_id)?;
        self.entered += 1;
        let result = self.instance.bindings.call_dispatch(
            &mut self.instance.store,
            uuid_to_id(method_id),
            arg,
        );
        self.entered -= 1;
        let result = match result {
            Ok(result) => result,
            Err(e) => {
                let message = match e.downcast_ref::<wasmtime::Trap>() {
                    Some(wasmtime::Trap::Interrupt) => {
                        limits::overran(&self.name, method_id, self.budget)
                    }
                    _ => format!("dispatch trapped: {e:#?}"),
                };
                self.state = State::Trapped(message.clone());
                return Err(DispatchError::Trap { message });
            }
        };
        match result {
            // A guest that writes its error as an error buffer, the form Arora
            // Buffers give an error, has it read as its error.
            Ok(bytes) => match guest_error(&bytes) {
                Some(message) => Err(DispatchError::Guest { message }),
                None => Ok(bytes.into_boxed_slice()),
            },
            Err(message) => Err(DispatchError::Guest { message }),
        }
    }
}

/// What a component that trapped answers a call made while its instance that
/// trapped is still in use.
const TRAPPED: &str = "the module trapped in an earlier call and runs no call until the call \
                       to it under way returns; its next call after that instantiates it afresh";

/// What a component that trapped, and then failed to instantiate afresh,
/// answers each later call with.
const LOST: &str = "the module trapped in an earlier call and failed to instantiate afresh: \
                    it runs no more calls until it is reloaded";

#[cfg(test)]
mod tests {
    use super::*;

    /// A component exporting `f`, of type `func {kind}(result u32)`, lifted
    /// with the canonical options `lift_options`.
    fn component(kind: &str, lift_options: &str) -> String {
        format!(
            r#"(component
                (core module $m
                    (func (export "f") (result i32) i32.const 0)
                    (func (export "cb") (param i32 i32 i32) (result i32) i32.const 0))
                (core instance $i (instantiate $m))
                (func (export "f") {kind} (result u32)
                    (canon lift (core func $i "f") {lift_options})))"#
        )
    }

    /// A component lifting an export with the async ABI does not compile.
    #[test]
    fn a_component_using_the_async_abi_is_refused() {
        let engine = WasmEngine::new(&config()).unwrap();
        let async_lifted = component("async", r#"async (callback (core func $i "cb"))"#);
        let error = Component::new(&engine, async_lifted).err().unwrap();
        assert!(
            format!("{error:#}").contains("require the component model async feature"),
            "{error:#}"
        );
    }

    /// The same export lifted synchronously compiles.
    #[test]
    fn a_component_lifting_synchronously_compiles() {
        let engine = WasmEngine::new(&config()).unwrap();
        Component::new(&engine, component("", "")).unwrap();
    }
}
