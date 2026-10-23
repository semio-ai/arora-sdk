use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::OnceLock;

use anyhow::Result;
use bytes::Buf;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::{
    AsContextMut, Caller, Config, Engine as WasmEngine, Extern, Linker, Memory,
    Module as WasmModule, Store, TypedFunc,
};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use arora_buffers::serde_uuid::serialize;
use arora_types::module::low::{ExportSymbol, ModuleDefinition};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::compiled::{Code, CompiledModule};
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
            WasmEngine::new(&config).map_err(|e| format!("{e:#}"))
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
        exports: &[ExportSymbol],
        module: WasmModule,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let arora_engine = self.arora_engine.ok_or_else(|| {
            LoadModuleError::Internal("WebAssemblyExecutor: set_engine not called".into())
        })?;
        let engine = module.engine().clone();
        let state = HostState {
            wasi: WasiCtxBuilder::new().inherit_stdio().build_p1(),
            engine: EnginePtr(arora_engine),
            malloc: None,
        };

        let store = Store::new(&engine, state);

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut HostState| &mut s.wasi)
            .map_err(|err| {
                LoadModuleError::Internal(format!("failed to map to wasm linker: {}", err))
            })?;

        let module = WebAssemblyModule::new(exports, module, store, linker)
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
        let module = compile(&module_definition.executable)?;
        self.instantiate(&module_definition.header.exports, module)
    }

    fn load_compiled_module(
        &mut self,
        module: &CompiledModule,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        match &module.code {
            Code::Wasmtime(compiled) => self.instantiate(&module.header.exports, compiled.clone()),
            Code::Executable(executable) => self.load_module(module_definition_from_parts(
                module.header.clone(),
                executable.as_ref().into(),
            )),
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
}

impl WebAssemblyModule {
    fn arora_dispatch(
        mut caller: Caller<'_, HostState>,
        module_id: u32,
        method_id: u32,
        arg: u32,
    ) -> u32 {
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

        let result_addr = malloc.call(&mut context, (result.len() as u32,)).unwrap();

        memory
            .write(&mut context, result_addr as usize, &result)
            .unwrap();

        result_addr
    }

    fn arora_dispatch_indirect(mut caller: Caller<'_, HostState>, callable_id: u64) -> u32 {
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

        let result_addr = malloc
            .call(&mut context, (result_buffer.len() as u32,))
            .unwrap();

        memory
            .write(&mut context, result_addr as usize, &result_buffer)
            .unwrap();

        result_addr
    }

    pub fn new(
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
        })
    }

    fn malloc(&mut self, size: u32) -> Result<u32, DispatchError> {
        self.malloc
            .call(&mut self.store, (size,))
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
}

impl Module for WebAssemblyModule {
    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
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
}
