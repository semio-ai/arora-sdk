//! A guest module compiled once and loaded into any number of engines.

use std::fmt;
use std::sync::Arc;

use crate::executor::LoadModuleError;
use crate::schema::module::low::Header;

/// A guest module compiled once, to load into any number of engines: its
/// [`Header`] and its executable, in the form the executor the header names
/// instantiates from.
///
/// - For the WebAssembly executors (`"wasm"`, `"wasm-component"`),
///   [`new`](Self::new) compiles the executable: natively (the
///   `wasmtime-host` feature) to a `wasmtime::Module` or a
///   `wasmtime::component::Component`, on `wasm32` to a `WebAssembly.Module`
///   — for a component, of the guest's core module in it. Each engine that
///   loads it creates its own instance — the guest's memory and globals — from
///   that compiled code, without compiling again.
/// - For any other executor (a native library), it holds the executable's
///   bytes, shared between loads; that executor loads them exactly as it
///   loads a [`ModuleDefinition`](crate::schema::module::low::ModuleDefinition).
///
/// Load it with [`Engine::load_compiled_module`](crate::engine::Engine::load_compiled_module).
/// Cloning is cheap: the header is cloned, the compiled code is shared.
/// Natively it is `Send + Sync`, so a module compiled on one thread loads on
/// any other.
#[derive(Clone)]
pub struct CompiledModule {
    pub(crate) header: Header,
    pub(crate) code: Code,
}

/// What an executor instantiates a [`CompiledModule`] from.
#[derive(Clone)]
pub(crate) enum Code {
    /// Compiled by wasmtime, on the process-wide engine the WebAssembly
    /// executor runs every instance in.
    #[cfg(all(feature = "wasmtime-host", not(target_arch = "wasm32")))]
    Wasmtime(wasmtime::Module),
    /// A component compiled by wasmtime, on the process-wide engine the
    /// component executor runs every instance in.
    #[cfg(all(feature = "wasmtime-host", not(target_arch = "wasm32")))]
    Component(wasmtime::component::Component),
    /// Compiled by the browser's `WebAssembly` runtime.
    #[cfg(target_arch = "wasm32")]
    Browser(js_sys::WebAssembly::Module),
    /// A component's guest core module, compiled by the browser's
    /// `WebAssembly` runtime.
    #[cfg(target_arch = "wasm32")]
    BrowserComponent(crate::executor::browser::BrowserComponent),
    /// The executable's bytes, for an executor that loads them as they are.
    Executable(Arc<[u8]>),
}

impl CompiledModule {
    /// Compile `executable` for the executor `header` names.
    ///
    /// A `"wasm"` or `"wasm-component"` module is compiled here, when this
    /// build carries an executor for it. Natively, an executable that is not
    /// valid WebAssembly, or a core module that exceeds what the `wasm`
    /// executor's pooling allocator holds per instance (one memory, one
    /// table), fails with [`LoadModuleError::MalformedExecutable`]; on
    /// `wasm32`, one that does not
    /// compile — or a component the browser executor does not run, see
    /// [`BrowserComponentExecutor`](crate::executor::browser::BrowserComponentExecutor)
    /// — fails with [`LoadModuleError::Internal`] carrying the reason. Any other module keeps its bytes, which its executor reads
    /// when the module is loaded. Whether the executable exports what the
    /// header declares is checked when the module is loaded, as for a
    /// [`ModuleDefinition`](crate::schema::module::low::ModuleDefinition).
    ///
    /// On `wasm32` the compilation is synchronous, so the browser's limit on
    /// synchronous compilation on the main thread applies (8 MB in Chrome).
    pub fn new(header: Header, executable: &[u8]) -> Result<Self, LoadModuleError> {
        let code = match header.executor.name.as_str() {
            WASM_EXECUTOR => compile_wasm(executable)?,
            COMPONENT_EXECUTOR => compile_component(executable)?,
            _ => Code::Executable(executable.into()),
        };
        Ok(Self { header, code })
    }

    /// The module's header, as given to [`new`](Self::new).
    pub fn header(&self) -> &Header {
        &self.header
    }
}

impl fmt::Debug for CompiledModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledModule")
            .field("header", &self.header)
            .finish_non_exhaustive()
    }
}

/// The name the WebAssembly executors (wasmtime natively, the browser's on
/// `wasm32`) register under.
const WASM_EXECUTOR: &str = "wasm";

/// The name the component executors register under.
const COMPONENT_EXECUTOR: &str = "wasm-component";

#[cfg(all(feature = "wasmtime-host", not(target_arch = "wasm32")))]
fn compile_component(executable: &[u8]) -> Result<Code, LoadModuleError> {
    crate::executor::component::compile(executable).map(Code::Component)
}

#[cfg(target_arch = "wasm32")]
fn compile_component(executable: &[u8]) -> Result<Code, LoadModuleError> {
    crate::executor::browser::compile_component(executable).map(Code::BrowserComponent)
}

/// No component executor in this build: the bytes are kept for the executor
/// registered under `"wasm-component"`.
#[cfg(not(any(feature = "wasmtime-host", target_arch = "wasm32")))]
fn compile_component(executable: &[u8]) -> Result<Code, LoadModuleError> {
    Ok(Code::Executable(executable.into()))
}

#[cfg(all(feature = "wasmtime-host", not(target_arch = "wasm32")))]
fn compile_wasm(executable: &[u8]) -> Result<Code, LoadModuleError> {
    crate::executor::wasm::compile(executable).map(Code::Wasmtime)
}

#[cfg(target_arch = "wasm32")]
fn compile_wasm(executable: &[u8]) -> Result<Code, LoadModuleError> {
    crate::executor::browser::compile(executable).map(Code::Browser)
}

/// No WebAssembly executor in this build: the bytes are kept, and an executor
/// registered under `"wasm"` receives them as it receives a module definition.
#[cfg(not(any(feature = "wasmtime-host", target_arch = "wasm32")))]
fn compile_wasm(executable: &[u8]) -> Result<Code, LoadModuleError> {
    Ok(Code::Executable(executable.into()))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// A compiled module is shared across threads natively, as the engines that
    /// load it may live on different ones.
    #[test]
    fn is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CompiledModule>();
    }
}
