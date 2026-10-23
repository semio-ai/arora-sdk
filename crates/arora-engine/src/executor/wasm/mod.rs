use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::OnceLock;

use anyhow::Result;
use bytes::Buf;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::{
    AsContextMut, Caller, Config, Engine as WasmEngine, Extern, Linker, Memory,
    Module as WasmModule, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use arora_buffers::serde_uuid::serialize;
use arora_types::module::low::{ExportSymbol, Header, ModuleDefinition};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::compiled::{Code, CompiledModule};
use crate::limits::{self, GuestLimits};
use crate::load::module_definition_from_parts;
use crate::{
    engine::EngineRef,
    executor::wasm::guest::AroraBuffer,
    module::{error_buffer, guest_error, DispatchError, Module},
};

mod guest;
use guest::ReadWasmMemory;

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
    Internal(#[error(not(source))] anyhow::Error),
}

/// [`EngineRef`] wrapper satisfying the `Send` bound that
/// [`wasmtime_wasi::p1::add_to_linker_sync`] puts on store data.
///
/// SAFETY: the engine and the modules holding this pointer live and run on a
/// single thread; `Send` is only claimed to satisfy the linker bound, the
/// pointer never actually crosses threads.
struct EnginePtr(EngineRef);
unsafe impl Send for EnginePtr {}

/// Host-side state held by each module's [`Store`].
struct HostState {
    wasi: WasiP1Ctx,

    /// The [`crate::engine::Engine`] this module belongs to. Guest calls to
    /// `arora_dispatch` re-enter the engine while it is already mutably
    /// borrowed further up the stack, which is why this is a raw pointer
    /// rather than a safe reference (see [`EngineRef`]).
    engine: EnginePtr,

    /// The guest's `arora_buffer_alloc` export, used by dispatch handlers to
    /// allocate result buffers in guest memory. Set right after instantiation.
    malloc: Option<TypedFunc<(u32,), u32>>,

    /// What the instance may grow its memory to ([`GuestLimits::memory`]).
    limits: StoreLimits,
}

/// The wasmtime engine every WebAssembly module compiles on and every instance
/// runs in: one per process, created on first use.
///
/// A `wasmtime::Module` instantiates only in a store of the engine that
/// compiled it, so a module compiled once can serve every device only if every
/// device runs on that engine. Each instance still has a store of its own, so
/// guests share no state; the engine holds compiled code and the instance
/// allocator. A failure to create it is kept: every later executor and
/// compilation fails with it.
fn engine() -> Result<&'static WasmEngine, LoadModuleError> {
    static ENGINE: OnceLock<Result<WasmEngine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            config.debug_info(cfg!(debug_assertions));
            config.cranelift_opt_level(wasmtime::OptLevel::Speed);
            config.allocation_strategy(wasmtime::InstanceAllocationStrategy::pooling());
            // A call's deadline (`crate::limits`) is checked against the
            // engine's epoch, which `limits::tick` advances.
            config.epoch_interruption(true);
            let engine = WasmEngine::new(&config).map_err(|e| format!("{e:#}"))?;
            limits::tick(&engine);
            Ok(engine)
        })
        .as_ref()
        .map_err(|e| {
            LoadModuleError::Internal(format!("failed to create the wasmtime engine: {e}"))
        })
}

/// Compile `executable` on the process-wide [`engine`].
pub(crate) fn compile(executable: &[u8]) -> Result<WasmModule, LoadModuleError> {
    WasmModule::new(engine()?, executable).map_err(|_| LoadModuleError::MalformedExecutable)
}

pub struct WebAssemblyExecutor {
    arora_engine: Option<EngineRef>,
}

impl WebAssemblyExecutor {
    /// An executor on the process-wide wasmtime engine, which this creates if
    /// no executor has yet. Fails if the engine cannot be created.
    pub fn new() -> Result<Self, InitializationError> {
        engine().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { arora_engine: None })
    }

    /// Instantiate `module` in a store of its own, linked to this executor's
    /// Arora engine, and look up the `exports` the header declares.
    fn instantiate(
        &self,
        header: &Header,
        module: WasmModule,
        limits: GuestLimits,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let exports = &header.exports;
        let arora_engine = self.arora_engine.ok_or_else(|| {
            LoadModuleError::Internal("WebAssemblyExecutor: set_engine not called".into())
        })?;
        let engine = module.engine().clone();
        let state = HostState {
            wasi: WasiCtxBuilder::new().inherit_stdio().build_p1(),
            engine: EnginePtr(arora_engine),
            malloc: None,
            limits: StoreLimitsBuilder::new().memory_size(limits.memory).build(),
        };

        let mut store = Store::new(&engine, state);
        store.limiter(|state| &mut state.limits);
        // Instantiating runs the guest's `_initialize`.
        store.set_epoch_deadline(limits::epochs_until(
            std::time::Instant::now() + limits::DEFAULT_CALL_BUDGET,
        ));

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut HostState| &mut s.wasi)
            .map_err(|err| {
                LoadModuleError::Internal(format!("failed to map to wasm linker: {}", err))
            })?;

        let module = WebAssemblyModule::new(&header.name, exports, module, store, linker)
            .map_err(|err| LoadModuleError::Internal(format!("failed to instantiate: {err:#}")))?;
        Ok(Box::new(module))
    }
}

impl Executor for WebAssemblyExecutor {
    fn set_engine(&mut self, engine: EngineRef) {
        self.arora_engine = Some(engine);
    }

    fn name(&self) -> &'static str {
        "wasm"
    }

    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        self.load_module_limited(module_definition, GuestLimits::default())
    }

    fn load_compiled_module(
        &mut self,
        module: &CompiledModule,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        self.load_compiled_module_limited(module, GuestLimits::default())
    }

    fn load_module_limited(
        &mut self,
        module_definition: ModuleDefinition,
        limits: GuestLimits,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let module = compile(&module_definition.executable)?;
        self.instantiate(&module_definition.header, module, limits)
    }

    fn load_compiled_module_limited(
        &mut self,
        module: &CompiledModule,
        limits: GuestLimits,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        match &module.code {
            Code::Wasmtime(compiled) => self.instantiate(&module.header, compiled.clone(), limits),
            Code::Executable(executable) => self.load_module_limited(
                module_definition_from_parts(module.header.clone(), executable.as_ref().into()),
                limits,
            ),
            _ => Err(LoadModuleError::Internal(
                "WebAssemblyExecutor: the module was compiled for another executor".into(),
            )),
        }
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), UnloadModuleError> {
        // The instance is dropped along with the WebAssemblyModule held by
        // the engine; nothing else to do.
        Ok(())
    }
}

/// Maintains references to a web assembly module,
/// and to the buffers used to exchange data with it.
struct WebAssemblyModule {
    /// The WASM module.
    module: WasmModule,

    /// Malloc function provided by the module.
    malloc: TypedFunc<(u32,), u32>,

    /// Free function provided by the module.
    free: TypedFunc<(u32,), ()>,

    /// Map of functions exported for arora.
    arora_functions: HashMap<Uuid, TypedFunc<(u32,), u32>>,

    /// The chunk of memory currently allocated to pass arguments as (ptr, size).
    current_arg_memory: Option<(u32, u32)>,

    /// The memory pool taken by the WASM module.
    memory: Memory,

    store: Store<HostState>,

    /// The module's name in its header, which an overrun's error names.
    header_name: String,
    /// The budget of the call under way, which an overrun's error names.
    budget: std::time::Duration,
    /// Whether a guest call of the dispatch under way was interrupted at its
    /// deadline: set where the call traps, read once the dispatch returns.
    interrupted: std::cell::Cell<bool>,
    /// Why the module refuses calls, once a call overran: an interrupted call
    /// can leave the guest's state half updated, so the module runs no more
    /// calls until it is reloaded.
    overran: Option<String>,
}

impl WebAssemblyModule {
    fn arora_dispatch(
        mut caller: Caller<'_, HostState>,
        module_id: u32,
        method_id: u32,
        arg: u32,
    ) -> wasmtime::Result<u32> {
        println!(
            "arora_dispatch: module_id: {}, method_id: {}, arg: {}",
            module_id, method_id, arg
        );

        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .unwrap();

        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let engine = unsafe { &mut *caller.data().engine.0 };
        let malloc = caller.data().malloc.as_ref().unwrap().clone();

        let mut context = caller.as_context_mut();

        let module_id = Uuid::read_wasm_memory(&context, memory, module_id);
        let method_id = Uuid::read_wasm_memory(&context, memory, method_id);
        let arg = AroraBuffer::read_wasm_memory(&context, memory, arg);

        // A call that fails reaches the calling guest as an error buffer, as a
        // guest's own error does: a refused argument or a callee's error is
        // the caller's to handle, never the host's to panic on.
        let result = engine
            .dispatch(&module_id, &method_id, arg.as_ref())
            .unwrap_or_else(|error| error_buffer(&error.to_string()));

        // The calling guest is past its deadline when the call outlasted it:
        // allocating the result then traps it, as its own code would.
        let result_addr = malloc.call(&mut context, (result.len() as u32,))?;

        memory.write(&mut context, result_addr as usize, &result)?;

        Ok(result_addr)
    }

    fn arora_dispatch_indirect(
        mut caller: Caller<'_, HostState>,
        callable_id: u64,
    ) -> wasmtime::Result<u32> {
        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let arora_caller: &mut dyn CallBridge = unsafe { &mut *caller.data().engine.0 };
        // As in `arora_dispatch`: a call that fails is an error buffer.
        let result_buffer = match arora_caller.arora_call_indirect(&CallableId { id: callable_id })
        {
            Ok(value) => serialize(&value),
            Err(error) => error_buffer(&error.to_string()),
        };

        let malloc = caller.data().malloc.as_ref().unwrap().clone();

        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .unwrap();

        let mut context = caller.as_context_mut();

        // As in `arora_dispatch`: past the deadline, the allocation traps.
        let result_addr = malloc.call(&mut context, (result_buffer.len() as u32,))?;

        memory.write(&mut context, result_addr as usize, &result_buffer)?;

        Ok(result_addr)
    }

    pub fn new(
        name: &str,
        exports: &[ExportSymbol],
        module: WasmModule,
        mut store: Store<HostState>,
        mut linker: Linker<HostState>,
    ) -> Result<Self> {
        linker.func_wrap("env", "arora_dispatch", Self::arora_dispatch)?;
        linker.func_wrap(
            "env",
            "arora_dispatch_indirect",
            Self::arora_dispatch_indirect,
        )?;

        let instance = linker.instantiate(&mut store, &module)?;

        let arora_buffer_free =
            instance.get_typed_func::<(u32,), ()>(&mut store, "arora_buffer_free")?;

        let arora_buffer_alloc =
            instance.get_typed_func::<(u32,), u32>(&mut store, "arora_buffer_alloc")?;

        let mut arora_functions = HashMap::new();
        for export in exports {
            let arora_function = instance.get_typed_func::<(u32,), u32>(
                &mut store,
                &format!(
                    "arora_function_{}",
                    export.id().to_string().replace('-', "_")
                ),
            )?;
            arora_functions.insert(*export.id(), arora_function);
        }

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("guest does not export 'memory'"))?;
        store.data_mut().malloc = Some(arora_buffer_alloc.clone());

        // A WASI reactor (a C or C++ guest linked with
        // `-mexec-model=reactor`) exports `_initialize`, which runs its static
        // constructors: it is called once, before any other export.
        if let Some(initialize) = instance.get_func(&mut store, "_initialize") {
            initialize
                .typed::<(), ()>(&store)?
                .call(&mut store, ())
                .map_err(|e| anyhow::Error::from(e).context("running the guest's _initialize"))?;
        }

        Ok(Self {
            module,
            store,
            malloc: arora_buffer_alloc,
            free: arora_buffer_free,
            arora_functions,
            memory,
            current_arg_memory: None,
            header_name: name.to_string(),
            budget: limits::DEFAULT_CALL_BUDGET,
            interrupted: std::cell::Cell::new(false),
            overran: None,
        })
    }

    fn malloc(&mut self, size: u32) -> Result<u32, DispatchError> {
        self.malloc
            .call(&mut self.store, (size,))
            .inspect_err(|e| self.note_interrupt(e))
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to allocate memory for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })
    }

    fn free(&mut self, addr: u32) -> Result<(), DispatchError> {
        self.free
            .call(&mut self.store, (addr,))
            .inspect_err(|e| self.note_interrupt(e))
            .map_err(|e| DispatchError::Trap {
                message: format!("failed to free memory for module {}: {:#?}", self.name(), e),
            })
    }

    fn allocate_arg_memory(&mut self, size: u32) -> Result<u32, DispatchError> {
        let ptr = self.malloc(size)?;
        self.current_arg_memory = Some((ptr, size));
        Ok(ptr)
    }

    /// Returns the module name if specified, else <unknown>.
    fn name(&self) -> &str {
        self.module.name().unwrap_or("<unknown>")
    }

    /// Note that a guest call trapped at its deadline, if `error` says so.
    fn note_interrupt(&self, error: &wasmtime::Error) {
        if let Some(wasmtime::Trap::Interrupt) = error.downcast_ref::<wasmtime::Trap>() {
            self.interrupted.set(true);
        }
    }

    fn dispatch_unbounded(
        &mut self,
        method_id: &Uuid,
        arg: &[u8],
    ) -> Result<Box<[u8]>, DispatchError> {
        // A function the header does not declare is refused before anything
        // is allocated in the guest.
        let func = self
            .arora_functions
            .get(method_id)
            .cloned()
            .ok_or(DispatchError::FunctionNotFound { id: *method_id })?;
        let arg_size = u32::try_from(arg.len()).map_err(|_| DispatchError::Internal {
            message: format!("failed to cast args size to u32 in module {}", self.name()),
        })?;

        // Let the WASM module allocate a buffer,
        // and copy the argument into it.
        let arg_addr = self.allocate_arg_memory(arg_size)?;
        self.memory
            .write(&mut self.store, arg_addr as usize, arg)
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to write to memory for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        // Calling the function. It returns the address of the buffer of the result.
        let result = func
            .call(&mut self.store, (arg_addr,))
            .inspect_err(|e| self.note_interrupt(e))
            .map_err(|e| DispatchError::Trap {
                message: format!("error calling {}.{}: {:#?}", self.name(), method_id, e),
            })?;

        // Free the buffer allocated for the argument.
        self.free(arg_addr)?;

        // Read the size of the result.
        let mut size_buffer = [0u8; 4];
        self.memory
            .read(&self.store, result as usize, &mut size_buffer)
            .map_err(|e| DispatchError::Internal {
                message: format!(
                    "failed to read the size of the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;
        let size = size_buffer.as_slice().get_u32_le();

        // Read the result into a local buffer.
        let mut result_buffer = vec![0u8; size as usize];
        self.memory
            .read(&self.store, result as usize, &mut result_buffer)
            .map_err(|e| DispatchError::Internal {
                message: format!(
                    "failed to read the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        // Free the result.
        self.free
            .call(&mut self.store, (result,))
            .inspect_err(|e| self.note_interrupt(e))
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to free the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        if let Some(message) = guest_error(&result_buffer) {
            return Err(DispatchError::Guest { message });
        }

        Ok(result_buffer.into_boxed_slice())
    }
}

impl Module for WebAssemblyModule {
    fn set_call_deadline(&mut self, deadline: std::time::Instant, budget: std::time::Duration) {
        self.store
            .set_epoch_deadline(limits::epochs_until(deadline));
        self.budget = budget;
    }

    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        if !self.arora_functions.contains_key(method_id) {
            return Err(DispatchError::FunctionNotFound { id: *method_id });
        }
        if let Some(overran) = &self.overran {
            return Err(DispatchError::Trap {
                message: format!("{OVERRAN} ({overran})"),
            });
        }
        self.interrupted.set(false);
        let result = self.dispatch_unbounded(method_id, arg);
        if self.interrupted.take() {
            let message = limits::overran(&self.header_name, method_id, self.budget);
            self.overran = Some(message.clone());
            return Err(DispatchError::Trap { message });
        }
        result
    }
}

/// What a core module whose call overran answers each later call with.
pub(crate) const OVERRAN: &str =
    "the module overran its budget in an earlier call and runs no more calls until it is reloaded";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineBuilder;
    use arora_types::module::low::{Executor as ExecutorName, Header};
    use arora_types::SemanticVersion;

    /// A guest declaring no function: a memory and the buffer allocator.
    const GUEST: &str = r#"(module
        (memory (export "memory") 1)
        (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "arora_buffer_free") (param i32)))"#;

    /// A call to a function the module's header does not declare fails as a
    /// function the module does not have.
    #[test]
    fn a_function_the_header_does_not_declare_is_not_found() {
        let module_id = Uuid::from_u128(1);
        let header = Header {
            id: module_id,
            name: "empty".to_string(),
            author: String::new(),
            description: None,
            license: String::new(),
            version: SemanticVersion {
                major: 1,
                minor: 0,
                patch: 0,
            },
            executor: ExecutorName {
                name: "wasm".to_string(),
                min_version: None,
                max_version: None,
            },
            exports: Vec::new(),
            imports: Vec::new(),
            executable_mime: "application/wasm".to_string(),
        };
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        engine
            .load_compiled_module(&CompiledModule::new(header, GUEST.as_bytes()).expect("compiles"))
            .expect("loads");

        let undeclared = Uuid::from_u128(2);
        assert!(matches!(
            engine.dispatch(&module_id, &undeclared, &[]),
            Err(DispatchError::FunctionNotFound { id }) if id == undeclared
        ));
    }

    /// A WASI reactor guest: `_initialize` adds one to the last byte of the
    /// result buffer `count` returns, so the result tells how many times it
    /// ran.
    const REACTOR: &str = r#"(module
        (memory (export "memory") 1)
        (data (i32.const 2048) "\05\00\00\00\00")
        (func (export "_initialize")
            (i32.store8 (i32.const 2052)
                (i32.add (i32.load8_u (i32.const 2052)) (i32.const 1))))
        (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "arora_buffer_free") (param i32))
        (func (export "arora_function_00000000_0000_0000_0000_000000000003")
            (param i32) (result i32) i32.const 2048))"#;

    /// A guest's `_initialize` runs once, when it loads, before its first
    /// call — not again before each call.
    #[test]
    fn a_reactor_guest_is_initialized_once_when_it_loads() {
        let module_id = Uuid::from_u128(1);
        let count = Uuid::from_u128(3);
        let header: Header = serde_yaml::from_str(&format!(
            r#"
id: {module_id}
name: reactor
author: ""
description: ~
license: ""
version: {{ major: 1, minor: 0, patch: 0 }}
executor: {{ name: wasm, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
  - type: function
    id: {count}
    name: count
    parameters: []
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
"#
        ))
        .expect("parse the header");
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        engine
            .load_compiled_module(
                &CompiledModule::new(header, REACTOR.as_bytes()).expect("compiles"),
            )
            .expect("loads");

        for _ in 0..2 {
            let result = engine
                .dispatch(&module_id, &count, &[])
                .expect("dispatches");
            assert_eq!(&*result, &[5, 0, 0, 0, 1]);
        }
    }

    /// The wat data string of `bytes`.
    fn wat_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("\\{byte:02x}")).collect()
    }

    /// A core module calling another through `arora_dispatch` with an
    /// argument of another type than the callee's parameter gets the
    /// engine's refusal back as an error buffer, which it returns as its own
    /// result: the host does not panic, and the callee is never entered.
    #[test]
    fn a_refused_call_between_core_modules_is_the_caller_s_error_buffer() {
        use arora_types::value::{Structure, StructureField, Value};
        let (caller, call) = (Uuid::from_u128(0xc0_0001), Uuid::from_u128(0xc0_0002));
        let (callee, take, count) = (
            Uuid::from_u128(0xc0_0003),
            Uuid::from_u128(0xc0_0004),
            Uuid::from_u128(0xc0_0005),
        );
        let header = |id: Uuid, function: Uuid, parameters: &str| -> Header {
            serde_yaml::from_str(&format!(
                r#"
id: {id}
name: m{id}
author: ""
description: ~
license: ""
version: {{ major: 1, minor: 0, patch: 0 }}
executor: {{ name: wasm, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
  - type: function
    id: {function}
    name: f
    parameters: {parameters}
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
"#
            ))
            .expect("parse the header")
        };
        // `take(count: u32)`, given a string.
        let wrong = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {
            id: take,
            fields: vec![StructureField {
                id: count,
                value: Box::new(Value::String("three".into())),
            }],
        }));
        let caller_guest = format!(
            r#"(module
                (import "env" "arora_dispatch" (func $dispatch (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 256) "{callee}")
                (data (i32.const 272) "{take}")
                (data (i32.const 512) "{wrong}")
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 4096)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{function}") (param i32) (result i32)
                    (call $dispatch (i32.const 256) (i32.const 272) (i32.const 512))))"#,
            callee = wat_bytes(callee.as_bytes()),
            take = wat_bytes(take.as_bytes()),
            wrong = wat_bytes(&wrong),
            function = call.to_string().replace('-', "_"),
        );
        // Entering the callee traps: a call that reaches it fails otherwise.
        let callee_guest = format!(
            r#"(module
                (memory (export "memory") 1)
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{function}") (param i32) (result i32) unreachable))"#,
            function = take.to_string().replace('-', "_"),
        );
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        for (header, guest) in [
            (header(caller, call, "[]"), caller_guest),
            (
                header(
                    callee,
                    take,
                    &format!(
                        "[{{ id: {count}, name: count, type: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000008 }} }}]"
                    ),
                ),
                callee_guest,
            ),
        ] {
            let compiled = CompiledModule::new(header, guest.as_bytes()).expect("compiles");
            engine.load_compiled_module(&compiled).expect("loads");
        }
        let refused = engine
            .dispatch(&caller, &call, &[])
            .expect_err("the caller returns the refusal as its error");
        assert!(
            matches!(&refused, DispatchError::Guest { message }
                if message.contains("the argument 'count' of 'f' is not of its declared type")),
            "{refused:?}"
        );
    }

    /// A core module calling a callable through `arora_dispatch_indirect` that
    /// fails gets the failure back as an error buffer, as through
    /// `arora_dispatch`.
    #[test]
    fn a_failing_indirect_call_is_the_caller_s_error_buffer() {
        use crate::call::{CallError, Callable};
        use arora_types::value::Value;
        struct Refusing;
        impl Callable for Refusing {
            fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, CallError> {
                Err(CallError::Generic {
                    message: "the callable refuses".into(),
                })
            }
        }
        let (module, call) = (Uuid::from_u128(0xc1_0001), Uuid::from_u128(0xc1_0002));
        let header: Header = serde_yaml::from_str(&format!(
            r#"
id: {module}
name: indirect
author: ""
description: ~
license: ""
version: {{ major: 1, minor: 0, patch: 0 }}
executor: {{ name: wasm, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
  - type: function
    id: {call}
    name: f
    parameters: []
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
"#
        ))
        .expect("parse the header");
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        let callable = engine.arora_register_callable(std::rc::Rc::new(Refusing));
        let guest = format!(
            r#"(module
                (import "env" "arora_dispatch_indirect" (func $indirect (param i64) (result i32)))
                (memory (export "memory") 1)
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 4096)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{function}") (param i32) (result i32)
                    (call $indirect (i64.const {callable}))))"#,
            function = call.to_string().replace('-', "_"),
            callable = callable.id,
        );
        let compiled = CompiledModule::new(header, guest.as_bytes()).expect("compiles");
        engine.load_compiled_module(&compiled).expect("loads");
        let refused = engine
            .dispatch(&module, &call, &[])
            .expect_err("the caller returns the failure as its error");
        assert!(
            matches!(&refused, DispatchError::Guest { message } if message.contains("the callable refuses")),
            "{refused:?}"
        );
    }

    /// A header for module `id`, named `name`, exporting `functions`, each
    /// taking no parameter.
    fn functions(id: Uuid, name: &str, functions: &[Uuid]) -> Header {
        let exports: String = functions
            .iter()
            .map(|function| {
                format!(
                    "  - type: function\n    id: {function}\n    name: f{n}\n    \
                     parameters: []\n    \
                     ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}\n",
                    n = function.as_u128(),
                )
            })
            .collect();
        serde_yaml::from_str(&format!(
            r#"
id: {id}
name: {name}
author: ""
description: ~
license: ""
version: {{ major: 1, minor: 0, patch: 0 }}
executor: {{ name: wasm, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
{exports}"#
        ))
        .expect("parse the header")
    }

    /// A header for module `id` exporting `function`, taking no parameter.
    fn one_function(id: Uuid, function: Uuid) -> Header {
        serde_yaml::from_str(&format!(
            r#"
id: {id}
name: limited
author: ""
description: ~
license: ""
version: {{ major: 1, minor: 0, patch: 0 }}
executor: {{ name: wasm, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
  - type: function
    id: {function}
    name: f
    parameters: []
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
"#
        ))
        .expect("parse the header")
    }

    /// A guest whose `function` loops forever.
    fn spinning(function: Uuid) -> String {
        format!(
            r#"(module
                (memory (export "memory") 1)
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{f}") (param i32) (result i32)
                    (loop $forever (br $forever))
                    i32.const 0))"#,
            f = function.to_string().replace('-', "_"),
        )
    }

    /// A call that loops forever is interrupted within its budget and one
    /// epoch, whether the budget is the module's own or the time left in the
    /// step; the module then refuses calls until it is reloaded.
    #[test]
    fn a_call_that_overruns_its_budget_is_interrupted_and_the_module_refuses_calls() {
        use crate::limits::{GuestLimits, EPOCH};
        use std::time::{Duration, Instant};
        let (module, spin) = (Uuid::from_u128(0xd0_0001), Uuid::from_u128(0xd0_0002));
        let compiled =
            CompiledModule::new(one_function(module, spin), spinning(spin).as_bytes()).unwrap();
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        engine.load_compiled_module(&compiled).expect("loads");
        // Slack for a loaded machine: the deadline itself is the budget plus
        // at most one epoch.
        let slack = Duration::from_millis(200) + EPOCH;

        let budget = Duration::from_millis(50);
        engine.set_guest_limits(
            module,
            GuestLimits {
                call_budget: Some(budget),
                ..GuestLimits::default()
            },
        );
        let started = Instant::now();
        let overran = engine.dispatch(&module, &spin, &[]).unwrap_err();
        let took = started.elapsed();
        assert!(took >= budget && took < budget + slack, "{took:?}");
        assert!(
            matches!(&overran, DispatchError::Trap { message }
                if message.contains("of 'limited' overran its budget of 50 ms")),
            "{overran:?}"
        );
        let refused = engine.dispatch(&module, &spin, &[]).unwrap_err();
        assert!(
            matches!(&refused, DispatchError::Trap { message } if message.contains(OVERRAN)),
            "{refused:?}"
        );

        // Reloaded, it runs again, now under the time left in the step.
        engine.reload_module(&compiled).expect("reloads");
        engine.set_guest_limits(module, GuestLimits::default());
        let step = Duration::from_millis(30);
        engine.set_step_budget(Some(step));
        let started = Instant::now();
        let overran = engine.dispatch(&module, &spin, &[]).unwrap_err();
        let took = started.elapsed();
        engine.set_step_budget(None);
        assert!(took >= step && took < step + slack, "{took:?}");
        assert!(
            matches!(&overran, DispatchError::Trap { message } if message.contains("overran")),
            "{overran:?}"
        );
    }

    /// A guest growing its memory past its cap sees the grow fail, returning
    /// -1, and goes on; under the cap, the grow succeeds.
    #[test]
    fn a_guest_growing_its_memory_past_its_cap_sees_the_grow_fail() {
        use crate::limits::GuestLimits;
        let (module, grow) = (Uuid::from_u128(0xd1_0001), Uuid::from_u128(0xd1_0002));
        // `grow` grows the memory by 32 pages (2 MiB) and writes, in a
        // five-byte buffer, the grow's result's low byte.
        let guest = format!(
            r#"(module
                (memory (export "memory") 1)
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{f}") (param i32) (result i32)
                    (i32.store (i32.const 2048) (i32.const 5))
                    (i32.store8 (i32.const 2052) (memory.grow (i32.const 32)))
                    i32.const 2048))"#,
            f = grow.to_string().replace('-', "_"),
        );
        let compiled = CompiledModule::new(one_function(module, grow), guest.as_bytes()).unwrap();
        for (memory, grown) in [(1 << 20, 0xff), (4 << 20, 1)] {
            let mut engine = EngineBuilder::new()
                .with_guest_limits(GuestLimits {
                    memory,
                    ..GuestLimits::default()
                })
                .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
                .build();
            engine.load_compiled_module(&compiled).expect("loads");
            let result = engine
                .dispatch(&module, &grow, &[])
                .expect("the call returns");
            assert_eq!(&*result, &[5, 0, 0, 0, grown], "a cap of {memory} bytes");
            // A reloaded module is under the same cap.
            engine.reload_module(&compiled).expect("reloads");
            let result = engine
                .dispatch(&module, &grow, &[])
                .expect("the call returns");
            assert_eq!(
                &*result,
                &[5, 0, 0, 0, grown],
                "reloaded, a cap of {memory} bytes"
            );
        }
    }
    /// A call without arguments to `function`, as a guest writes one.
    fn no_arguments(function: Uuid) -> String {
        use arora_types::value::{Structure, Value};
        wat_bytes(&arora_buffers::serde_uuid::serialize(&Value::Structure(
            Structure {
                id: function,
                fields: vec![],
            },
        )))
    }

    /// A guest calling another through `arora_dispatch`, when the callee
    /// outlasts the caller's deadline, is interrupted once the call returns:
    /// its call fails, it does not take the host down, and both modules
    /// refuse calls until they are reloaded.
    #[test]
    fn a_guest_whose_callee_outlasts_its_deadline_is_interrupted() {
        use crate::limits::EPOCH;
        use std::time::{Duration, Instant};
        let (caller, call) = (Uuid::from_u128(0xd3_0001), Uuid::from_u128(0xd3_0002));
        let (callee, spin) = (Uuid::from_u128(0xd3_0003), Uuid::from_u128(0xd3_0004));
        let caller_guest = format!(
            r#"(module
                (import "env" "arora_dispatch" (func $dispatch (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 256) "{callee}")
                (data (i32.const 272) "{spin}")
                (data (i32.const 512) "{arguments}")
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 4096)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{call}") (param i32) (result i32)
                    (call $dispatch (i32.const 256) (i32.const 272) (i32.const 512))))"#,
            callee = wat_bytes(callee.as_bytes()),
            spin = wat_bytes(spin.as_bytes()),
            arguments = no_arguments(spin),
            call = call.to_string().replace('-', "_"),
        );
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        for (header, guest) in [
            (functions(caller, "caller", &[call]), caller_guest),
            (functions(callee, "callee", &[spin]), spinning(spin)),
        ] {
            let compiled = CompiledModule::new(header, guest.as_bytes()).expect("compiles");
            engine.load_compiled_module(&compiled).expect("loads");
        }
        // Only the caller has a budget: the callee's call ends by it.
        let budget = Duration::from_millis(30);
        engine.set_guest_limits(
            caller,
            crate::limits::GuestLimits {
                call_budget: Some(budget),
                ..crate::limits::GuestLimits::default()
            },
        );
        let started = Instant::now();
        let overran = engine.dispatch(&caller, &call, &[]).unwrap_err();
        let took = started.elapsed();
        assert!(
            took >= budget && took < budget + 2 * EPOCH + Duration::from_millis(200),
            "{took:?}"
        );
        assert!(
            matches!(&overran, DispatchError::Trap { message }
                if message.contains("of 'caller' overran its budget")),
            "{overran:?}"
        );
        for (module, function) in [(caller, call), (callee, spin)] {
            let refused = engine.dispatch(&module, &function, &[]).unwrap_err();
            assert!(
                matches!(&refused, DispatchError::Trap { message } if message.contains(OVERRAN)),
                "{refused:?}"
            );
        }
    }

    /// A guest calling itself through `arora_dispatch` does not extend its
    /// call's deadline: a loop of calls back into it is interrupted at its
    /// budget.
    #[test]
    fn a_guest_calling_itself_keeps_its_call_s_deadline() {
        use crate::limits::{GuestLimits, EPOCH};
        use std::time::{Duration, Instant};
        let (module, again, answer) = (
            Uuid::from_u128(0xd4_0001),
            Uuid::from_u128(0xd4_0002),
            Uuid::from_u128(0xd4_0003),
        );
        // `again` calls `answer` of its own module, forever; `answer` returns
        // a four-byte buffer at once.
        let guest = format!(
            r#"(module
                (import "env" "arora_dispatch" (func $dispatch (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 256) "{module}")
                (data (i32.const 272) "{answer}")
                (data (i32.const 512) "{arguments}")
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 4096)
                (func (export "arora_buffer_free") (param i32))
                (func (export "arora_function_{again_name}") (param i32) (result i32)
                    (loop $forever
                        (drop (call $dispatch (i32.const 256) (i32.const 272) (i32.const 512)))
                        (br $forever))
                    i32.const 0)
                (func (export "arora_function_{answer_name}") (param i32) (result i32)
                    (i32.store (i32.const 2048) (i32.const 4))
                    i32.const 2048))"#,
            module = wat_bytes(module.as_bytes()),
            answer = wat_bytes(answer.as_bytes()),
            arguments = no_arguments(answer),
            again_name = again.to_string().replace('-', "_"),
            answer_name = answer.to_string().replace('-', "_"),
        );
        let budget = Duration::from_millis(50);
        let mut engine = EngineBuilder::new()
            .with_guest_limits(GuestLimits {
                call_budget: Some(budget),
                ..GuestLimits::default()
            })
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        let compiled = CompiledModule::new(
            functions(module, "reentrant", &[again, answer]),
            guest.as_bytes(),
        )
        .expect("compiles");
        engine.load_compiled_module(&compiled).expect("loads");
        let started = Instant::now();
        let overran = engine.dispatch(&module, &again, &[]).unwrap_err();
        let took = started.elapsed();
        assert!(
            took >= budget && took < budget + 2 * EPOCH + Duration::from_millis(200),
            "{took:?}"
        );
        assert!(
            matches!(&overran, DispatchError::Trap { message }
                if message.contains("of 'reentrant' overran its budget of 50 ms")),
            "{overran:?}"
        );
    }

    /// A guest interrupted while freeing its result is interrupted as in its
    /// function: the module refuses calls until it is reloaded.
    #[test]
    fn a_guest_interrupted_freeing_its_result_refuses_calls() {
        use crate::limits::GuestLimits;
        use std::time::Duration;
        let (module, function) = (Uuid::from_u128(0xd5_0001), Uuid::from_u128(0xd5_0002));
        // Freeing the result, at 2048, never returns.
        let guest = format!(
            r#"(module
                (memory (export "memory") 1)
                (func (export "arora_buffer_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "arora_buffer_free") (param i32)
                    (if (i32.eq (local.get 0) (i32.const 2048))
                        (then (loop $forever (br $forever)))))
                (func (export "arora_function_{f}") (param i32) (result i32)
                    (i32.store (i32.const 2048) (i32.const 4))
                    i32.const 2048))"#,
            f = function.to_string().replace('-', "_"),
        );
        let mut engine = EngineBuilder::new()
            .with_guest_limits(GuestLimits {
                call_budget: Some(Duration::from_millis(50)),
                ..GuestLimits::default()
            })
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        let compiled =
            CompiledModule::new(functions(module, "freeing", &[function]), guest.as_bytes())
                .expect("compiles");
        engine.load_compiled_module(&compiled).expect("loads");
        let overran = engine.dispatch(&module, &function, &[]).unwrap_err();
        assert!(
            matches!(&overran, DispatchError::Trap { message }
                if message.contains("of 'freeing' overran its budget of 50 ms")),
            "{overran:?}"
        );
        let refused = engine.dispatch(&module, &function, &[]).unwrap_err();
        assert!(
            matches!(&refused, DispatchError::Trap { message } if message.contains(OVERRAN)),
            "{refused:?}"
        );
    }

    /// During a step, a module's own budget and the time left in the step
    /// bound its call together: whichever ends first.
    #[test]
    fn a_call_ends_by_the_earlier_of_the_step_and_its_module_s_budget() {
        use crate::limits::{GuestLimits, EPOCH};
        use std::time::{Duration, Instant};
        let (module, spin) = (Uuid::from_u128(0xd6_0001), Uuid::from_u128(0xd6_0002));
        let compiled =
            CompiledModule::new(one_function(module, spin), spinning(spin).as_bytes()).unwrap();
        let mut engine = EngineBuilder::new()
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        engine.load_compiled_module(&compiled).expect("loads");
        let (short, long) = (Duration::from_millis(30), Duration::from_millis(400));
        for (step, own) in [(long, short), (short, long)] {
            engine.reload_module(&compiled).expect("reloads");
            engine.set_guest_limits(
                module,
                GuestLimits {
                    call_budget: Some(own),
                    ..GuestLimits::default()
                },
            );
            engine.set_step_budget(Some(step));
            let started = Instant::now();
            let overran = engine.dispatch(&module, &spin, &[]).unwrap_err();
            let took = started.elapsed();
            engine.set_step_budget(None);
            assert!(
                took < short + 2 * EPOCH + Duration::from_millis(200),
                "step {step:?}, own {own:?}: {took:?}"
            );
            assert!(
                matches!(&overran, DispatchError::Trap { message } if message.contains("overran")),
                "{overran:?}"
            );
        }
    }

    /// The epoch counter follows the wall clock: a long budget is not
    /// stretched by the ticker's sleeps overrunning.
    #[test]
    fn a_long_budget_is_kept_to_the_wall_clock() {
        use crate::limits::GuestLimits;
        use std::time::{Duration, Instant};
        let (module, spin) = (Uuid::from_u128(0xd7_0001), Uuid::from_u128(0xd7_0002));
        let compiled =
            CompiledModule::new(one_function(module, spin), spinning(spin).as_bytes()).unwrap();
        let budget = Duration::from_millis(800);
        let mut engine = EngineBuilder::new()
            .with_guest_limits(GuestLimits {
                call_budget: Some(budget),
                ..GuestLimits::default()
            })
            .add_executor(WebAssemblyExecutor::new().expect("the wasm executor"))
            .build();
        engine.load_compiled_module(&compiled).expect("loads");
        let started = Instant::now();
        engine.dispatch(&module, &spin, &[]).unwrap_err();
        let took = started.elapsed();
        // Counting sleeps instead overruns by several percent: 50 ms and more
        // here.
        assert!(
            took >= budget && took < budget + Duration::from_millis(40),
            "{took:?}"
        );
    }
}
