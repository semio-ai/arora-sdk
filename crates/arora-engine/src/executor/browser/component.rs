//! Browser-hosted executor for `wasm-component` modules.
//!
//! The browser's `WebAssembly` runtime instantiates core modules, not
//! components. This executor instantiates the guest's own core module alone —
//! [`component_layout`](crate::component_layout) finds it, and refuses a
//! component laid out otherwise, or whose imports, exports and their wiring
//! the guest module alone does not stand for — and gives it, directly, the
//! canonical-ABI form of each function it imports. It calls the guest's
//! `_initialize` when the component has an initializer, as the component
//! would, then the guest's `functions` and `dispatch` exports through their
//! canonical-ABI form.
//!
//! The guest may import the `arora:module/host` interface and the WASI 0.2
//! interfaces the native component executor links: `wasi:cli` (environment,
//! exit, standard streams, terminals), `wasi:io`, `wasi:clocks`,
//! `wasi:random` and `wasi:filesystem`. Each behaves as it does in a browser
//! tab:
//!
//! - standard output and error go to the console, a line at a time; standard
//!   input is closed; no stream is a terminal; there are no environment
//!   variables, arguments or working directory;
//! - every pollable is ready, so nothing blocks: waiting on a clock returns at
//!   once;
//! - the monotonic clock is `performance.now()`, the wall clock `Date.now()`,
//!   and random bytes come from `crypto.getRandomValues`;
//! - `exit` stops the call that made it, as a trap;
//! - the filesystem has no directory, as natively: `get-directories` returns
//!   none, so no descriptor exists, and a call on one traps, as natively a call
//!   on an unknown handle does.
//!
//! A guest importing anything else fails to load, naming the import
//! ([`provided`](crate::component_layout::provided)). A component that traps
//! is entered no more: every later call fails with the trap it met, until the
//! module is reloaded, as natively.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Reflect, Uint8Array, WebAssembly};
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use arora_buffers::serde_uuid::serialize as serialize_value;
use arora_types::module::low::{ExportSymbol, ModuleDefinition};

use super::{js_to_load_err, Prepared, SharedLoaderRc};
use crate::call::{CallBridge, CallableId};
use crate::compiled::{Code, CompiledModule};
use crate::component_layout::{guest_module, provided};
use crate::engine::EngineRef;
use crate::executor::{Executor, LoadModuleError, UnloadModuleError};
use crate::load::module_definition_from_parts;
use crate::module::{guest_error, DispatchError, Module};

/// Loads `wasm-component` modules in the browser. Built on a
/// [`BrowserExecutor`](super::BrowserExecutor)'s loader, so that modules staged
/// asynchronously through it ([`SharedLoader::prepare`](super::SharedLoader::prepare))
/// load here too.
pub struct BrowserComponentExecutor {
    shared: SharedLoaderRc,
}

impl BrowserComponentExecutor {
    /// An executor loading the modules `shared` stages, alongside the
    /// [`BrowserExecutor`](super::BrowserExecutor) it comes from.
    pub fn new(shared: SharedLoaderRc) -> Self {
        Self { shared }
    }

    fn engine(&self) -> Result<EngineRef, LoadModuleError> {
        self.shared.engine.get().ok_or_else(|| {
            LoadModuleError::Internal("BrowserComponentExecutor: set_engine not called".into())
        })
    }

    /// The instance staged for `id` by the loader, if there is one.
    fn staged(&self, id: Uuid) -> Result<Option<PreparedComponent>, LoadModuleError> {
        match self.shared.prepared.borrow_mut().remove(&id) {
            None => Ok(None),
            Some(Prepared::Component(prepared)) => Ok(Some(prepared)),
            Some(Prepared::Core(_)) => Err(LoadModuleError::Internal(format!(
                "module {id} was prepared as a core module, but its header names the \
                 wasm-component executor"
            ))),
        }
    }
}

impl Executor for BrowserComponentExecutor {
    fn set_engine(&mut self, engine: EngineRef) {
        self.shared.engine.set(Some(engine));
    }

    fn name(&self) -> &'static str {
        "wasm-component"
    }

    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let prepared = match self.staged(module_definition.header.id)? {
            Some(prepared) => prepared,
            None => instantiate(self.engine()?, &compile(&module_definition.executable)?)?,
        };
        bind(prepared, &module_definition.header.exports)
    }

    fn load_compiled_module(
        &mut self,
        module: &CompiledModule,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        match &module.code {
            Code::BrowserComponent(guest) => {
                let prepared = match self.staged(module.header.id)? {
                    Some(prepared) => prepared,
                    None => instantiate(self.engine()?, guest)?,
                };
                bind(prepared, &module.header.exports)
            }
            Code::Executable(executable) => self.load_module(module_definition_from_parts(
                module.header.clone(),
                executable.as_ref().into(),
            )),
            _ => Err(LoadModuleError::Internal(
                "BrowserComponentExecutor: the module was compiled for another executor".into(),
            )),
        }
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), UnloadModuleError> {
        // The instance and its imports are dropped along with the module the
        // engine holds; nothing else to do.
        Ok(())
    }
}

// --- compiling and instantiating ---------------------------------------------

/// Compile the guest module of `component` synchronously (`new
/// WebAssembly.Module`), subject to the browser's limit on synchronous
/// compilation on the main thread (8 MB in Chrome).
pub(crate) fn compile(component: &[u8]) -> Result<BrowserComponent, LoadModuleError> {
    let guest = guest_module(component).map_err(LoadModuleError::Internal)?;
    let module = WebAssembly::Module::new(&Uint8Array::from(guest.bytes).into())
        .map_err(|e| LoadModuleError::Internal(format!("WebAssembly.Module: {e:?}")))?;
    Ok(BrowserComponent {
        module,
        initialize: guest.initialize,
    })
}

/// A component compiled for the browser: its guest module, and whether the
/// component calls the guest's `_initialize` when it is instantiated.
#[derive(Clone)]
pub(crate) struct BrowserComponent {
    module: WebAssembly::Module,
    initialize: bool,
}

/// Compile and instantiate the guest module of `component` asynchronously
/// (`WebAssembly.compile`, `WebAssembly.instantiate`), which no main-thread
/// size limit applies to.
pub(crate) async fn prepare(
    engine: EngineRef,
    component: &[u8],
) -> Result<PreparedComponent, JsValue> {
    let guest = guest_module(component).map_err(|e| JsValue::from_str(&e))?;
    let module: WebAssembly::Module =
        JsFuture::from(WebAssembly::compile(&Uint8Array::from(guest.bytes).into()))
            .await?
            .dyn_into()
            .map_err(|_| JsValue::from_str("WebAssembly.compile: no module in result"))?;
    let imports = Imports::new(engine, &module).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let instance: WebAssembly::Instance =
        JsFuture::from(WebAssembly::instantiate_module(&module, &imports.object))
            .await?
            .dyn_into()
            .map_err(|_| JsValue::from_str("WebAssembly.instantiate: no instance in result"))?;
    Ok(imports.with_instance(instance, guest.initialize))
}

/// Instantiate `component`'s guest module synchronously (`new
/// WebAssembly.Instance`).
fn instantiate(
    engine: EngineRef,
    component: &BrowserComponent,
) -> Result<PreparedComponent, LoadModuleError> {
    let imports = Imports::new(engine, &component.module)?;
    let instance = WebAssembly::Instance::new(&component.module, &imports.object)
        .map_err(|e| LoadModuleError::Internal(format!("WebAssembly.Instance: {e:?}")))?;
    Ok(imports.with_instance(instance, component.initialize))
}

/// An instantiated guest module and what its imports hold on to, staged for
/// the synchronous tail of a load.
pub(crate) struct PreparedComponent {
    instance: WebAssembly::Instance,
    /// Whether the guest's `_initialize` is to run before its first call.
    initialize: bool,
    host: Rc<Host>,
    closures: Vec<Box<dyn Any>>,
}

/// Bind the guest's exports, checking that it serves every function its header
/// declares (`declared`), completing a load.
fn bind(
    prepared: PreparedComponent,
    declared: &[ExportSymbol],
) -> Result<Box<dyn Module>, LoadModuleError> {
    let PreparedComponent {
        instance,
        initialize,
        host,
        closures,
    } = prepared;
    let exports = instance.exports();
    let export = |name: &str| -> Result<Option<Function>, LoadModuleError> {
        let value = Reflect::get(&exports, &name.into()).map_err(js_to_load_err)?;
        Ok(value.dyn_into::<Function>().ok())
    };
    let required = |name: &str| {
        export(name)?
            .ok_or_else(|| LoadModuleError::Internal(format!("the guest does not export '{name}'")))
    };
    let memory: WebAssembly::Memory = Reflect::get(&exports, &"memory".into())
        .map_err(js_to_load_err)?
        .dyn_into()
        .map_err(|_| LoadModuleError::Internal("the guest does not export 'memory'".into()))?;
    let guest = Guest {
        memory,
        realloc: required("cabi_realloc")?,
    };
    *host.guest.borrow_mut() = Some(guest.clone());

    // The component's initializer runs the guest's `_initialize` (a WASI
    // reactor's static constructors) once, before anything else is called.
    if initialize {
        required("_initialize")?
            .call0(&JsValue::NULL)
            .map_err(|e| {
                LoadModuleError::Internal(format!("_initialize trapped: {}", js_message(e)))
            })?;
    }

    let functions = required("functions")?;
    let served = call_functions(&guest, &functions, export("cabi_post_functions")?.as_ref())
        .map_err(|e| LoadModuleError::Internal(format!("functions trapped: {e}")))?;
    let function_ids: Vec<Uuid> = declared.iter().map(|e| *e.id()).collect();
    if let Some(missing) = function_ids.iter().find(|id| !served.contains(id)) {
        return Err(LoadModuleError::Internal(format!(
            "failed to find function export {missing}: the component does not serve it"
        )));
    }

    Ok(Box::new(ComponentModule {
        guest,
        dispatch: required("dispatch")?,
        post_dispatch: export("cabi_post_dispatch")?,
        function_ids,
        trapped: None,
        _instance: instance,
        _host: host,
        _closures: closures,
    }))
}

/// The ids the guest's `functions` export returns: a `list<id>`, each id two
/// `u64` halves.
fn call_functions(
    guest: &Guest,
    functions: &Function,
    post_return: Option<&Function>,
) -> Result<Vec<Uuid>, String> {
    let ret = as_u32(functions.call0(&JsValue::NULL).map_err(js_message)?)?;
    let ptr = guest.read_u32(ret)?;
    let len = guest.read_u32(ret + 4)?;
    let bytes = guest.read(
        ptr,
        len.checked_mul(16)
            .ok_or("the list of functions overflows")?,
    )?;
    let ids = bytes
        .chunks_exact(16)
        .map(|id| {
            let hi = u64::from_le_bytes(id[..8].try_into().expect("8 bytes"));
            let lo = u64::from_le_bytes(id[8..].try_into().expect("8 bytes"));
            Uuid::from_u64_pair(hi, lo)
        })
        .collect();
    if let Some(post_return) = post_return {
        post_return
            .call1(&JsValue::NULL, &JsValue::from(ret))
            .map_err(js_message)?;
    }
    Ok(ids)
}

struct ComponentModule {
    guest: Guest,
    dispatch: Function,
    post_dispatch: Option<Function>,
    /// Functions the module's header declares; dispatch refuses others.
    function_ids: Vec<Uuid>,
    /// Why a call trapped, once one has. A component is entered no more after
    /// a trap, as wasmtime enforces natively: its state is whatever the trap
    /// left. The module says so, naming the trap, until it is reloaded.
    trapped: Option<String>,
    // The instance and its imports live as long as the module.
    _instance: WebAssembly::Instance,
    _host: Rc<Host>,
    _closures: Vec<Box<dyn Any>>,
}

impl Module for ComponentModule {
    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        if !self.function_ids.contains(method_id) {
            return Err(DispatchError::FunctionNotFound { id: *method_id });
        }
        if let Some(trap) = &self.trapped {
            return Err(DispatchError::Trap {
                message: format!("{TRAPPED} ({trap})"),
            });
        }
        let result = self.call_dispatch(method_id, arg);
        if let Err(DispatchError::Trap { message }) = &result {
            self.trapped = Some(message.clone());
        }
        result
    }
}

/// What a component that trapped answers each later call with, as natively.
const TRAPPED: &str =
    "the module trapped in an earlier call and runs no more calls until it is reloaded";

impl ComponentModule {
    fn call_dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        let trap = |message: String| DispatchError::Trap {
            message: format!("dispatch trapped: {message}"),
        };
        // `dispatch(method: id, arg: list<u8>) -> result<list<u8>, string>`:
        // the id's halves and the list's pointer and length go in; a pointer to
        // the result comes back.
        let (hi, lo) = method_id.as_u64_pair();
        let arg_ptr = self.guest.alloc(1, arg).map_err(trap)?;
        let args = Array::of4(
            &JsValue::from(hi as i64),
            &JsValue::from(lo as i64),
            &JsValue::from(arg_ptr),
            &JsValue::from(arg.len() as u32),
        );
        let ret = self
            .dispatch
            .apply(&JsValue::NULL, &args)
            .map_err(js_message)
            .and_then(as_u32)
            .map_err(trap)?;
        let result = self.guest.read_result(ret).map_err(trap)?;
        if let Some(post_return) = &self.post_dispatch {
            post_return
                .call1(&JsValue::NULL, &JsValue::from(ret))
                .map_err(|e| trap(js_message(e)))?;
        }
        match result {
            // A guest that writes its error as an error buffer, as a core
            // module does, has it read as its error, as a core module's is.
            Ok(bytes) => match guest_error(&bytes) {
                Some(message) => Err(DispatchError::Guest { message }),
                None => Ok(bytes.into_boxed_slice()),
            },
            // A string that is not UTF-8 does not lift: wasmtime traps on it.
            Err(message) => match String::from_utf8(message) {
                Ok(message) => Err(DispatchError::Guest { message }),
                Err(_) => Err(trap("the error string is not UTF-8".into())),
            },
        }
    }
}

// --- the guest's memory ------------------------------------------------------

/// The guest's memory and allocator, which the canonical ABI reads and writes
/// arguments and results through.
#[derive(Clone)]
struct Guest {
    memory: WebAssembly::Memory,
    realloc: Function,
}

impl Guest {
    fn view(&self) -> Uint8Array {
        // A fresh view each time: the memory's buffer changes when it grows.
        Uint8Array::new(&self.memory.buffer())
    }

    fn range(&self, ptr: u32, len: u32) -> Result<(Uint8Array, u32, u32), String> {
        let view = self.view();
        match ptr.checked_add(len) {
            Some(end) if end <= view.length() => Ok((view, ptr, end)),
            _ => Err(format!(
                "{len} bytes at {ptr} are out of the guest's memory ({} bytes)",
                view.length()
            )),
        }
    }

    fn read(&self, ptr: u32, len: u32) -> Result<Vec<u8>, String> {
        let (view, start, end) = self.range(ptr, len)?;
        Ok(view.subarray(start, end).to_vec())
    }

    fn write(&self, ptr: u32, bytes: &[u8]) -> Result<(), String> {
        let (view, start, _) = self.range(ptr, bytes.len() as u32)?;
        view.set(&Uint8Array::from(bytes), start);
        Ok(())
    }

    fn read_u32(&self, ptr: u32) -> Result<u32, String> {
        let bytes = self.read(ptr, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
    }

    /// Allocate `bytes` in the guest, aligned to `align`, and copy them there.
    fn alloc(&self, align: u32, bytes: &[u8]) -> Result<u32, String> {
        let args = Array::of4(
            &JsValue::from(0),
            &JsValue::from(0),
            &JsValue::from(align),
            &JsValue::from(bytes.len() as u32),
        );
        let ptr = self
            .realloc
            .apply(&JsValue::NULL, &args)
            .map_err(js_message)
            .and_then(as_u32)?;
        self.write(ptr, bytes)?;
        Ok(ptr)
    }

    /// Store a list of `bytes` (elements aligned to `align`) as a pointer and a
    /// length at `ret`.
    fn store_list(&self, ret: u32, align: u32, bytes: &[u8], len: u32) -> Result<(), String> {
        let ptr = self.alloc(align, bytes)?;
        self.write(ret, &ptr.to_le_bytes())?;
        self.write(ret + 4, &len.to_le_bytes())
    }

    /// Store a `result<list<u8>, string>` at `ret`: its case, then the list.
    fn store_result(&self, ret: u32, result: Result<Vec<u8>, String>) -> Result<(), String> {
        let (case, bytes) = match result {
            Ok(bytes) => (0, bytes),
            Err(message) => (1, message.into_bytes()),
        };
        self.write(ret, &[case])?;
        self.store_list(ret + 4, 1, &bytes, bytes.len() as u32)
    }

    /// Read a `result<list<u8>, string>` at `ret`, the string as its bytes.
    fn read_result(&self, ret: u32) -> Result<Result<Vec<u8>, Vec<u8>>, String> {
        let case = self.read(ret, 1)?[0];
        let bytes = self.read(self.read_u32(ret + 4)?, self.read_u32(ret + 8)?)?;
        match case {
            0 => Ok(Ok(bytes)),
            1 => Ok(Err(bytes)),
            other => Err(format!("a result's case is {other}")),
        }
    }
}

fn as_u32(value: JsValue) -> Result<u32, String> {
    value
        .as_f64()
        .map(|n| n as i64 as u32)
        .ok_or_else(|| format!("expected an i32, got {value:?}"))
}

fn js_message(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .unwrap_or_else(|| format!("{e:?}"))
}

// --- the imports -------------------------------------------------------------

/// What the guest's imports share: the engine, and the guest's memory once
/// the instance exists.
struct Host {
    engine: EngineRef,
    guest: RefCell<Option<Guest>>,
    stdout: RefCell<Vec<u8>>,
    stderr: RefCell<Vec<u8>>,
}

impl Host {
    /// The guest's memory. Imports run only once the guest does, after
    /// [`bind`] has set it.
    fn guest(&self) -> Guest {
        self.guest
            .borrow()
            .clone()
            .unwrap_or_else(|| wasm_bindgen::throw_str("the guest ran before it was bound"))
    }

    /// The buffer of the output stream `handle`.
    fn output(&self, handle: i32) -> &RefCell<Vec<u8>> {
        match handle {
            STDOUT => &self.stdout,
            STDERR => &self.stderr,
            _ => trap(format!("{handle} is not an output stream")),
        }
    }

    /// Append `bytes` to the output stream `handle`, logging each line it
    /// completes; with `flush`, log what remains too.
    fn write(&self, handle: i32, bytes: &[u8], flush: bool) {
        let mut buffer = self.output(handle).borrow_mut();
        buffer.extend_from_slice(bytes);
        let mut lines: Vec<String> = Vec::new();
        while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            lines.push(String::from_utf8_lossy(&line[..end]).into_owned());
        }
        if flush && !buffer.is_empty() {
            lines.push(String::from_utf8_lossy(&buffer).into_owned());
            buffer.clear();
        }
        drop(buffer);
        for line in lines {
            if handle == STDERR {
                web_sys::console::error_1(&line.into());
            } else {
                web_sys::console::log_1(&line.into());
            }
        }
    }
}

/// The handles of the standard streams. A pollable's handle is [`READY`].
const STDIN: i32 = 1;
const STDOUT: i32 = 2;
const STDERR: i32 = 3;
const READY: i32 = 1;

/// What an output stream's `check-write` permits at once.
const WRITE_BUDGET: u64 = 1 << 16;

/// Stop the guest: a trap, raised as a JavaScript exception through the
/// guest's frames.
fn trap(message: String) -> ! {
    wasm_bindgen::throw_str(&message)
}

/// The guest's import object, and the closures behind it.
struct Imports {
    object: Object,
    host: Rc<Host>,
    closures: Vec<Box<dyn Any>>,
}

impl Imports {
    /// Provide every function `module` imports, or fail naming the first this
    /// host does not provide.
    fn new(engine: EngineRef, module: &WebAssembly::Module) -> Result<Self, LoadModuleError> {
        let mut imports = Self {
            object: Object::new(),
            host: Rc::new(Host {
                engine,
                guest: RefCell::new(None),
                stdout: RefCell::new(Vec::new()),
                stderr: RefCell::new(Vec::new()),
            }),
            closures: Vec::new(),
        };
        let mut namespaces: HashMap<String, Object> = HashMap::new();
        for import in WebAssembly::Module::imports(module).iter() {
            let field = |key: &str| {
                Reflect::get(&import, &key.into())
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_default()
            };
            let (namespace, name, kind) = (field("module"), field("name"), field("kind"));
            let function = (kind == "function")
                .then(|| imports.function(&namespace, &name))
                .flatten()
                .ok_or_else(|| {
                    LoadModuleError::Internal(format!(
                        "the guest imports the {kind} '{name}' from '{namespace}', which the \
                         browser does not provide"
                    ))
                })?;
            let target = namespaces.entry(namespace.clone()).or_insert_with(|| {
                let target = Object::new();
                Reflect::set(&imports.object, &namespace.as_str().into(), &target)
                    .expect("setting a property of a fresh object");
                target
            });
            Reflect::set(target, &name.as_str().into(), &function).map_err(js_to_load_err)?;
        }
        Ok(imports)
    }

    fn with_instance(self, instance: WebAssembly::Instance, initialize: bool) -> PreparedComponent {
        PreparedComponent {
            instance,
            initialize,
            host: self.host,
            closures: self.closures,
        }
    }

    /// Keep `closure` for as long as the guest and return its function.
    fn keep<T: ?Sized + 'static>(&mut self, closure: Closure<T>) -> JsValue {
        let function = closure.as_ref().clone();
        self.closures.push(Box::new(closure));
        function
    }

    /// The canonical-ABI form of the function `name` of the interface
    /// `namespace` (`package:name/interface@version`), if this host provides
    /// it.
    fn function(&mut self, namespace: &str, name: &str) -> Option<JsValue> {
        if !provided(namespace) {
            return None;
        }
        let (interface, _) = namespace.split_once('@')?;
        let host = self.host.clone();
        Some(match (interface, name) {
            ("arora:module/host", "dispatch") => {
                self.keep(Closure::<dyn Fn(i64, i64, i64, i64, i32, i32, i32)>::new(
                    move |module_hi, module_lo, method_hi, method_lo, arg, arg_len, ret| {
                        let guest = host.guest();
                        let arg = guest
                            .read(arg as u32, arg_len as u32)
                            .unwrap_or_else(|e| trap(e));
                        let module = Uuid::from_u64_pair(module_hi as u64, module_lo as u64);
                        let method = Uuid::from_u64_pair(method_hi as u64, method_lo as u64);
                        // SAFETY: re-entrant single-threaded access, like the
                        // native executors' `HostState::engine`.
                        let engine = unsafe { &mut *host.engine };
                        let result = engine
                            .dispatch(&module, &method, &arg)
                            .map(Vec::from)
                            .map_err(|e| format!("{e}"));
                        guest
                            .store_result(ret as u32, result)
                            .unwrap_or_else(|e| trap(e));
                    },
                ))
            }
            ("arora:module/host", "dispatch-indirect") => {
                self.keep(Closure::<dyn Fn(i64, i32)>::new(move |callable, ret| {
                    let guest = host.guest();
                    // SAFETY: re-entrant single-threaded access, like the
                    // native executors' `HostState::engine`.
                    let bridge: &mut dyn CallBridge = unsafe { &mut *host.engine };
                    let result = bridge
                        .arora_call_indirect(&CallableId {
                            id: callable as u64,
                        })
                        .map(|value| serialize_value(&value).into())
                        .map_err(|e| format!("{e}"));
                    guest
                        .store_result(ret as u32, result)
                        .unwrap_or_else(|e| trap(e));
                }))
            }

            // Resources: the handles this host gives out need no freeing.
            (
                "wasi:io/error"
                | "wasi:io/poll"
                | "wasi:io/streams"
                | "wasi:cli/terminal-input"
                | "wasi:cli/terminal-output",
                name,
            ) if name.starts_with("[resource-drop]") => {
                self.keep(Closure::<dyn Fn(i32)>::new(|_| {}))
            }
            ("wasi:io/error", "[method]error.to-debug-string") => {
                // No call this host serves fails with an error resource.
                self.keep(Closure::<dyn Fn(i32, i32)>::new(|handle, _| {
                    trap(format!("{handle} is not an error"))
                }))
            }

            // Every pollable is ready.
            ("wasi:io/poll", "[method]pollable.ready") => {
                self.keep(Closure::<dyn Fn(i32) -> i32>::new(|_| 1))
            }
            ("wasi:io/poll", "[method]pollable.block") => {
                self.keep(Closure::<dyn Fn(i32)>::new(|_| {}))
            }
            ("wasi:io/poll", "poll") => {
                // `poll(in: list<borrow<pollable>>) -> list<u32>`: all of them.
                self.keep(Closure::<dyn Fn(i32, i32, i32)>::new(move |_, len, ret| {
                    let indices: Vec<u8> = (0..len as u32).flat_map(u32::to_le_bytes).collect();
                    host.guest()
                        .store_list(ret as u32, 4, &indices, len as u32)
                        .unwrap_or_else(|e| trap(e));
                }))
            }

            // Standard input is closed: `result<_, stream-error>` with
            // `stream-error::closed`, its payload past the case at the
            // alignment of the largest payload (a list, or a `u64`).
            ("wasi:io/streams", "[method]input-stream.read")
            | ("wasi:io/streams", "[method]input-stream.blocking-read") => {
                self.keep(Closure::<dyn Fn(i32, i64, i32)>::new(move |_, _, ret| {
                    write_closed(&host.guest(), ret as u32, 4)
                }))
            }
            ("wasi:io/streams", "[method]input-stream.skip")
            | ("wasi:io/streams", "[method]input-stream.blocking-skip") => {
                self.keep(Closure::<dyn Fn(i32, i64, i32)>::new(move |_, _, ret| {
                    write_closed(&host.guest(), ret as u32, 8)
                }))
            }
            ("wasi:io/streams", "[method]output-stream.splice")
            | ("wasi:io/streams", "[method]output-stream.blocking-splice") => {
                // The only input stream is closed standard input.
                self.keep(Closure::<dyn Fn(i32, i32, i64, i32)>::new(
                    move |_, _, _, ret| write_closed(&host.guest(), ret as u32, 8),
                ))
            }
            ("wasi:io/streams", "[method]input-stream.subscribe")
            | ("wasi:io/streams", "[method]output-stream.subscribe") => {
                self.keep(Closure::<dyn Fn(i32) -> i32>::new(|_| READY))
            }

            // Standard output and error go to the console.
            ("wasi:io/streams", "[method]output-stream.check-write") => {
                // `result<u64, stream-error>`: ok, the budget after the case.
                self.keep(Closure::<dyn Fn(i32, i32)>::new(move |handle, ret| {
                    host.output(handle);
                    let guest = host.guest();
                    let ret = ret as u32;
                    guest
                        .write(ret, &[0])
                        .and_then(|()| guest.write(ret + 8, &WRITE_BUDGET.to_le_bytes()))
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            ("wasi:io/streams", "[method]output-stream.write")
            | ("wasi:io/streams", "[method]output-stream.blocking-write-and-flush") => {
                let flush = name.contains("flush");
                self.keep(Closure::<dyn Fn(i32, i32, i32, i32)>::new(
                    move |handle, ptr, len, ret| {
                        let guest = host.guest();
                        let bytes = guest
                            .read(ptr as u32, len as u32)
                            .unwrap_or_else(|e| trap(e));
                        host.write(handle, &bytes, flush);
                        guest.write(ret as u32, &[0]).unwrap_or_else(|e| trap(e));
                    },
                ))
            }
            ("wasi:io/streams", "[method]output-stream.write-zeroes")
            | ("wasi:io/streams", "[method]output-stream.blocking-write-zeroes-and-flush") => {
                let flush = name.contains("flush");
                self.keep(Closure::<dyn Fn(i32, i64, i32)>::new(
                    move |handle, len, ret| {
                        let len = (len as u64).min(WRITE_BUDGET) as usize;
                        host.write(handle, &vec![0; len], flush);
                        host.guest()
                            .write(ret as u32, &[0])
                            .unwrap_or_else(|e| trap(e));
                    },
                ))
            }
            ("wasi:io/streams", "[method]output-stream.flush")
            | ("wasi:io/streams", "[method]output-stream.blocking-flush") => {
                self.keep(Closure::<dyn Fn(i32, i32)>::new(move |handle, ret| {
                    host.write(handle, &[], true);
                    host.guest()
                        .write(ret as u32, &[0])
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            ("wasi:cli/stdin", "get-stdin") => self.keep(Closure::<dyn Fn() -> i32>::new(|| STDIN)),
            ("wasi:cli/stdout", "get-stdout") => {
                self.keep(Closure::<dyn Fn() -> i32>::new(|| STDOUT))
            }
            ("wasi:cli/stderr", "get-stderr") => {
                self.keep(Closure::<dyn Fn() -> i32>::new(|| STDERR))
            }
            ("wasi:cli/terminal-stdin", "get-terminal-stdin")
            | ("wasi:cli/terminal-stdout", "get-terminal-stdout")
            | ("wasi:cli/terminal-stderr", "get-terminal-stderr") => {
                // `option<terminal>`: none.
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    host.guest()
                        .write(ret as u32, &[0])
                        .unwrap_or_else(|e| trap(e));
                }))
            }

            // No environment, arguments or working directory.
            ("wasi:cli/environment", "get-environment")
            | ("wasi:cli/environment", "get-arguments") => {
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    host.guest()
                        .store_list(ret as u32, 4, &[], 0)
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            ("wasi:cli/environment", "initial-cwd") => {
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    host.guest()
                        .write(ret as u32, &[0])
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            ("wasi:cli/exit", "exit") => self.keep(Closure::<dyn Fn(i32)>::new(|status| {
                trap(format!(
                    "the guest exited ({})",
                    if status == 0 { "success" } else { "failure" }
                ))
            })),

            // Clocks. A clock's pollable is ready at once.
            ("wasi:clocks/monotonic-clock", "now") => {
                self.keep(Closure::<dyn Fn() -> i64>::new(|| monotonic_nanos() as i64))
            }
            ("wasi:clocks/monotonic-clock", "resolution") => {
                self.keep(Closure::<dyn Fn() -> i64>::new(|| {
                    MONOTONIC_RESOLUTION as i64
                }))
            }
            ("wasi:clocks/monotonic-clock", "subscribe-instant")
            | ("wasi:clocks/monotonic-clock", "subscribe-duration") => {
                self.keep(Closure::<dyn Fn(i64) -> i32>::new(|_| READY))
            }
            ("wasi:clocks/wall-clock", "now") => {
                // `datetime { seconds: u64, nanoseconds: u32 }`.
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    let millis = js_sys::Date::now() as u64;
                    let nanos = ((millis % 1000) * 1_000_000) as u32;
                    write_datetime(&host.guest(), ret as u32, millis / 1000, nanos);
                }))
            }
            ("wasi:clocks/wall-clock", "resolution") => {
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    write_datetime(&host.guest(), ret as u32, 0, 1_000_000)
                }))
            }

            // Random bytes, from `crypto.getRandomValues`.
            ("wasi:random/random", "get-random-bytes")
            | ("wasi:random/insecure", "get-insecure-random-bytes") => {
                self.keep(Closure::<dyn Fn(i64, i32)>::new(move |len, ret| {
                    let guest = host.guest();
                    // No more than the guest's memory holds.
                    let len = len as u64;
                    if len > u64::from(guest.view().length()) {
                        trap(format!("{len} random bytes do not fit the guest's memory"));
                    }
                    let bytes = random_bytes(len as usize);
                    guest
                        .store_list(ret as u32, 1, &bytes, bytes.len() as u32)
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            ("wasi:random/random", "get-random-u64")
            | ("wasi:random/insecure", "get-insecure-random-u64") => {
                self.keep(Closure::<dyn Fn() -> i64>::new(|| random_u64() as i64))
            }
            // A filesystem with no directory in it.
            ("wasi:filesystem/preopens", "get-directories") => {
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    host.guest()
                        .store_list(ret as u32, 4, &[], 0)
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            // `option<error-code>`: none, as no error this host gives out is
            // the filesystem's.
            ("wasi:filesystem/types", "filesystem-error-code") => {
                self.keep(Closure::<dyn Fn(i32, i32)>::new(move |_error, ret| {
                    host.guest()
                        .write(ret as u32, &[0])
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            // Every other function takes a descriptor or a directory stream,
            // of which none exists.
            ("wasi:filesystem/types", name) => {
                let name = name.to_string();
                self.keep(Closure::<dyn Fn()>::new(move || {
                    trap(format!(
                        "{name}: the host preopens no directory, so no descriptor exists"
                    ))
                }))
            }
            ("wasi:random/insecure-seed", "insecure-seed") => {
                // `tuple<u64, u64>`.
                self.keep(Closure::<dyn Fn(i32)>::new(move |ret| {
                    let guest = host.guest();
                    let ret = ret as u32;
                    guest
                        .write(ret, &random_u64().to_le_bytes())
                        .and_then(|()| guest.write(ret + 8, &random_u64().to_le_bytes()))
                        .unwrap_or_else(|e| trap(e));
                }))
            }
            _ => return None,
        })
    }
}

/// Store `err(stream-error::closed)` at `ret`, the error `payload_offset` past
/// the result's case.
fn write_closed(guest: &Guest, ret: u32, payload_offset: u32) {
    const ERR: u8 = 1;
    const CLOSED: u8 = 1;
    guest
        .write(ret, &[ERR])
        .and_then(|()| guest.write(ret + payload_offset, &[CLOSED]))
        .unwrap_or_else(|e| trap(e));
}

fn write_datetime(guest: &Guest, ret: u32, seconds: u64, nanoseconds: u32) {
    guest
        .write(ret, &seconds.to_le_bytes())
        .and_then(|()| guest.write(ret + 8, &nanoseconds.to_le_bytes()))
        .unwrap_or_else(|e| trap(e));
}

/// The resolution of `performance.now()` browsers promise at the least, in
/// nanoseconds: 100 µs.
const MONOTONIC_RESOLUTION: u64 = 100_000;

/// `performance.now()`, in nanoseconds.
fn monotonic_nanos() -> u64 {
    let now = Reflect::get(&js_sys::global(), &"performance".into())
        .and_then(|performance| {
            let now: Function = Reflect::get(&performance, &"now".into())?.dyn_into()?;
            now.call0(&performance)
        })
        .ok()
        .and_then(|now| now.as_f64())
        .unwrap_or_else(|| trap("the browser has no performance.now()".into()));
    (now * 1_000_000.0) as u64
}

/// `len` bytes from `crypto.getRandomValues`, which fills at most 64 KiB at a
/// time.
fn random_bytes(len: usize) -> Vec<u8> {
    let crypto = Reflect::get(&js_sys::global(), &"crypto".into())
        .unwrap_or_else(|_| trap("the browser has no crypto".into()));
    let fill: Function = Reflect::get(&crypto, &"getRandomValues".into())
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .unwrap_or_else(|| trap("the browser has no crypto.getRandomValues()".into()));
    let mut bytes = vec![0; len];
    for chunk in bytes.chunks_mut(1 << 16) {
        let array = Uint8Array::new_with_length(chunk.len() as u32);
        fill.call1(&crypto, &array)
            .unwrap_or_else(|e| trap(js_message(e)));
        array.copy_to(chunk);
    }
    bytes
}

fn random_u64() -> u64 {
    u64::from_le_bytes(random_bytes(8).try_into().expect("8 bytes"))
}
