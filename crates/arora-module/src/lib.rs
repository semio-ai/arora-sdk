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
//! `polly::record(parent)`, `polly::exports()`, `polly::client::say(…)` and
//! the marker type `polly::Module`, which implements [`AroraModule`]. A host
//! registers the module with `HostModule::of::<polly::Module>()`.
//!
//! **The executor is not the declaration's to name.** Only the step that
//! builds the artifact knows whether it is native or wasm, so `header` takes
//! it there. A module linked into the host has no header at all: it is
//! registered from its exports and described by its record.

pub use arora_module_macros::{declare_module, export, module, module_from_header};
pub use arora_types::module::declared::{AroraFunction, AroraModule};

/// What the generated code reaches through, so a module crate depends on this
/// crate alone. Not a public interface: its contents follow the macros.
#[doc(hidden)]
pub mod __rt {
    pub use arora_buffers as buffers;
    pub use arora_types as types;
}
