//! A guest module compiled once and loaded into any number of engines.

use std::fmt;
use std::sync::Arc;

use crate::executor::LoadModuleError;
use crate::schema::module::low::Header;

/// A guest module compiled once, to load into any number of engines: its
/// [`Header`] and its executable, in the form the executor the header names
/// instantiates from.
///
/// - For the WebAssembly executor (`"wasm"`), [`new`](Self::new) compiles the
///   executable: to a `wasmtime::Module` natively (the `wasmtime-host`
///   feature), to a `WebAssembly.Module` on `wasm32`. Each engine that loads
///   it creates its own instance — the guest's memory and globals — from that
///   compiled code, without compiling again.
/// - For any other executor (a native library, a WebAssembly component), it
///   holds the executable's bytes, shared between loads; that executor loads
///   them exactly as it loads a
///   [`ModuleDefinition`](crate::schema::module::low::ModuleDefinition).
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
    /// Compiled by the browser's `WebAssembly` runtime.
    #[cfg(target_arch = "wasm32")]
    Browser(js_sys::WebAssembly::Module),
    /// The executable's bytes, for an executor that loads them as they are.
    Executable(Arc<[u8]>),
}

impl CompiledModule {
    /// Compile `executable` for the executor `header` names.
    ///
    /// A `"wasm"` module is compiled here, when this build carries a
    /// WebAssembly executor. Natively, an executable that is not valid
    /// WebAssembly, or that exceeds what the executor's instance allocator
    /// holds per instance (one memory, one table), fails with
    /// [`LoadModuleError::MalformedExecutable`]; on `wasm32`, one that does not
    /// compile fails with [`LoadModuleError::Internal`] carrying the browser's
    /// message. Any other module keeps its bytes, which its executor reads
    /// when the module is loaded. Whether the executable exports what the
    /// header declares is checked when the module is loaded, as for a
    /// [`ModuleDefinition`](crate::schema::module::low::ModuleDefinition).
    ///
    /// On `wasm32` the compilation is synchronous, so the browser's limit on
    /// synchronous compilation on the main thread applies (8 MB in Chrome).
    pub fn new(header: Header, executable: &[u8]) -> Result<Self, LoadModuleError> {
        let code = if header.executor.name == WASM_EXECUTOR {
            compile_wasm(executable)?
        } else {
            Code::Executable(executable.into())
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
