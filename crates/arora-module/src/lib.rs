//! Write an Arora module in Rust.
//!
//! A module's interface — its id, its exported functions, their parameters,
//! each pinned by id — lives in the crate that implements it, declared with
//! macros on the Rust module and its functions, the way
//! [`AroraType`](arora_types::AroraType) lets a Rust type carry its schema.
//! From that one declaration come every other form the module takes: the
//! header a `module.yaml` is written from at export, the record a store
//! serves, the functions a host registers, the stubs a caller programs
//! against, and the entry points an executor looks up in an artifact.
//!
//! ```ignore
//! #[arora_module::module(id = "a1a6bb9a-…", name = "polly", version = "0.1.0")]
//! pub mod polly {
//!   #[export(id = "e1b4bda7-…")]
//!   pub fn say(#[param(id = "fb3787f2-…")] text: String) -> Status { … }
//! }
//! ```
//!
//! The declaration yields `polly::ids`, `polly::header(executor)`,
//! `polly::record(parent)`, `polly::type_records()` — the records of the
//! types its functions name, which the `#[derive(AroraType)]` types produce —
//! `polly::exports()`, `polly::client::say(…)` and the marker type
//! `polly::Module`, which implements [`AroraModule`]. A host registers the
//! module with `HostModule::of::<polly::Module>()`.
//!
//! `scope = SCOPE` names the
//! [`Scope`](arora_types::record::scope::Scope) the module's records live in
//! (`AroraModule::scope`): its own, and those of the types it names that
//! state no scope of their own. A module published to the store states one;
//! without it, its records live at the root.
//!
//! A **contract** declares functions that several modules implement, each
//! under its own module id — one `say` served by two speech providers, say.
//! It is a trait whose methods have no body and take `&mut self`: the
//! implementation the host module owns and calls them on, a zero-sized unit
//! struct when it keeps no state:
//!
//! ```ignore
//! #[arora_module::contract(name = "say")]
//! pub trait Say {
//!   #[export(id = "e1b4bda7-…")]
//!   fn say(&mut self, #[param(id = "fb3787f2-…")] text: String) -> Status;
//! }
//!
//! impl Say for Cloud { fn say(&mut self, text: String) -> Status { … } }
//! HostModule::from_exports(CLOUD_ID, say::exports(Cloud::new()));
//! ```
//!
//! The declaration yields `say::ids`, `say::NAME`, `say::descriptions()`,
//! `say::record(parent)`, `say::type_records(scope)` and
//! `say::exports(implementation)`. A contract has no artifact entry points:
//! an artifact exports one module's functions, declared with `#[module]`.
//!
//! **The executor is not the declaration's to name.** Only the step that
//! builds the artifact knows whether it is native or wasm, so `header` takes
//! it there. A module linked into the host has no header at all: it is
//! registered from its exports and described by its record.

// The bindings name this crate by its absolute path, as the module crates
// whose `#[module]` expands into them do.
extern crate self as arora_module;

pub use arora_module_macros::{contract, declare_module, export, module, module_from_header};
pub use arora_types::module::declared::{AroraFunction, AroraModule};

/// What the generated code reaches through, so a module crate depends on this
/// crate alone. Not a public interface: its contents follow the macros.
#[doc(hidden)]
pub mod __rt {
    pub use arora_buffers as buffers;
    pub use arora_types as types;
    #[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
    pub use wit_bindgen;

    /// The engine calls a core module imports (`env.arora_dispatch`,
    /// `env.arora_dispatch_indirect`), made through the `host` interface, for
    /// code written against the core-module ABI — arora-module-rust's — built
    /// as a component. Each takes and returns the addresses of size-prefixed
    /// buffers and leaks the one it returns, as the engine's imports do; an
    /// error the host returns comes back as an error buffer.
    #[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
    pub mod engine {
        use crate::component::arora::module::host;
        use crate::component::Id;

        /// # Safety
        ///
        /// `address` is that of a UUID's 16 bytes.
        unsafe fn id_at(address: usize) -> Id {
            let bytes = unsafe { *(address as *const [u8; 16]) };
            let (hi, lo) = arora_types::Uuid::from_bytes(bytes).as_u64_pair();
            Id { hi, lo }
        }

        /// # Safety
        ///
        /// `address` is that of a size-prefixed buffer.
        unsafe fn buffer_at<'a>(address: usize) -> &'a [u8] {
            unsafe {
                let size = u32::from_le_bytes(*(address as *const [u8; 4])) as usize;
                std::slice::from_raw_parts(address as *const u8, size)
            }
        }

        fn leak(result: Result<Vec<u8>, String>) -> usize {
            let buffer = match result {
                Ok(buffer) => buffer.into_boxed_slice(),
                Err(message) => {
                    let mut writer = arora_buffers::BufferWriter::new();
                    writer.add_error(&message);
                    writer.finalize()
                }
            };
            Box::leak(buffer).as_ptr() as usize
        }

        /// Call the function `method_id` of the module `module_id` with the
        /// argument buffer at `arg`.
        ///
        /// # Safety
        ///
        /// `module_id` and `method_id` are the addresses of UUIDs' 16 bytes,
        /// `arg` that of a size-prefixed buffer.
        pub unsafe extern "C" fn arora_dispatch(
            module_id: usize,
            method_id: usize,
            arg: usize,
        ) -> usize {
            let (module, method, arg) =
                unsafe { (id_at(module_id), id_at(method_id), buffer_at(arg)) };
            leak(host::dispatch(module, method, arg))
        }

        /// Call the callable `callable_id` registered with the engine.
        ///
        /// # Safety
        ///
        /// None beyond the core-module ABI's: the function is `unsafe` as the
        /// import it stands for is.
        pub unsafe extern "C" fn arora_dispatch_indirect(callable_id: u64) -> usize {
            leak(host::dispatch_indirect(callable_id))
        }
    }
}

/// Name the modules an artifact exports: `artifact!(polly)`, or several when
/// one artifact carries several modules — a device loads it once per module,
/// under each module's header. Written once, at the root of the crate built as
/// the artifact.
///
/// Built for `wasm32-wasip2`, the artifact is a component of the
/// `arora:module` world: `functions` lists the functions these modules
/// export, and `dispatch` routes a call to the function its method id names,
/// in whichever of them exports it.
/// Built otherwise, it expands to nothing: a core module or a shared library
/// exports each function under its own name.
#[macro_export]
macro_rules! artifact {
    ($($($segment:ident)::+),+ $(,)?) => {
        #[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
        const _: () = {
            struct Component;

            impl $crate::component::Guest for Component {
                fn functions() -> ::std::vec::Vec<$crate::component::Id> {
                    let mut functions = ::std::vec::Vec::new();
                    $(
                        for id in $($segment)::+::function_ids() {
                            let (hi, lo) = id.as_u64_pair();
                            functions.push($crate::component::Id { hi, lo });
                        }
                    )+
                    functions
                }

                fn dispatch(
                    method: $crate::component::Id,
                    arg: ::std::vec::Vec<u8>,
                ) -> ::std::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                    let method = $crate::__rt::types::Uuid::from_u64_pair(method.hi, method.lo);
                    $(
                        if let ::std::option::Option::Some(result) = $($segment)::+::dispatch_buffer(method, &arg) {
                            return result.map(::std::vec::Vec::from);
                        }
                    )+
                    ::std::result::Result::Err(::std::format!(
                        "the artifact exports no function {method}"
                    ))
                }
            }

            $crate::component::export!(Component);
        };
    };
}

/// The `arora:module` world (`wit/arora-module.wit`), guest side: what a
/// module built for `wasm32-wasip2` is a component of. [`artifact!`] implements
/// [`Guest`](component::Guest) and exports it; the host interface's imports,
/// [`dispatch`](component::arora::module::host::dispatch) and
/// [`dispatch_indirect`](component::arora::module::host::dispatch_indirect),
/// call other modules through the engine.
#[cfg(all(target_arch = "wasm32", target_os = "wasi", target_env = "p2"))]
pub mod component {
    wit_bindgen::generate!({
        world: "module",
        path: "wit",
        pub_export_macro: true,
        default_bindings_module: "::arora_module::component",
        runtime_path: "::arora_module::__rt::wit_bindgen::rt",
    });
}
