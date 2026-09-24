//! What a module declared in Rust implements — the twin of [`AroraType`] for
//! modules.
//!
//! A declaration (the `arora-module` macros) pins a module's id, its exported
//! functions and their parameter ids in Rust, and produces from them every
//! other form the module takes: the [`Header`] a `module.yaml` is written
//! from, the [`record`](frozen::Module) a store serves, and its exported
//! functions, callable. The marker type the declaration emits implements this
//! trait, so a host reaches all of that generically.
//!
//! [`AroraType`]: crate::AroraType

use crate::call::{Call, CallError, CallResult};
use crate::module::low::{Executor, Header};
use crate::record::module::frozen::{self, Function};
use crate::Uuid;

/// One function a declared module exports, callable: a [`Call`] in —
/// arguments matched by parameter id — and its [`CallResult`] out.
///
/// How it is reached depends on how the module is built, not on the function:
/// a host that links the module registers `invoke` directly; a module built
/// as an artifact (a wasm guest, a native shared library) wraps the same
/// `invoke` under the executor's buffer ABI.
pub struct AroraFunction {
  /// The function's id, what a call targets.
  pub id: Uuid,
  /// The function's name, what method introspection lists.
  pub name: &'static str,
  /// The frozen signature: parameters (name, type, order) and return type.
  pub signature: Function,
  /// The function itself.
  pub invoke: Box<dyn FnMut(Call) -> Result<CallResult, CallError>>,
}

/// A module declared in Rust.
pub trait AroraModule {
  /// The module's id.
  fn id() -> Uuid;

  /// The module's header: what a `module.yaml` declares, resolved. The
  /// executor is the exporter's to name — only the step that builds the
  /// artifact knows whether it is wasm or native — so it is given here. A
  /// module linked into the host has no header: it is registered from its
  /// [`exports`](Self::exports) and described by [`record`](Self::record).
  fn header(executor: Executor) -> Header;

  /// The module as a store record, under the folder `parent`.
  fn record(parent: Uuid) -> frozen::Module;

  /// Every exported function, callable.
  fn exports() -> Vec<AroraFunction>;
}
