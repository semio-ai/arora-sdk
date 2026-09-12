//! What a module declared in Rust implements — the twin of [`AroraType`] for
//! modules.
//!
//! A declaration (the `arora-module` macros) pins a module's id, its exported
//! functions and their parameter ids in Rust, and produces from them every
//! other form the module takes: the [`Header`] a `module.yaml` is written
//! from, the [`record`](frozen::Module) a store serves, and the host closures
//! a device registers. The marker type the declaration emits implements this
//! trait, so a host reaches all of that generically.
//!
//! [`AroraType`]: crate::AroraType

use crate::call::{Call, CallError, CallResult};
use crate::module::low::{Executor, Header};
use crate::record::module::frozen::{self, Function};
use crate::Uuid;

/// One exported function as a host registers it: its id, its name, its
/// frozen signature, and the closure that takes a [`Call`] — arguments
/// matched by parameter id — to its [`CallResult`].
pub type HostFunction = (
  Uuid,
  &'static str,
  Function,
  Box<dyn FnMut(Call) -> Result<CallResult, CallError>>,
);

/// A module declared in Rust.
pub trait AroraModule {
  /// The module's id.
  fn id() -> Uuid;

  /// The module's header: what a `module.yaml` declares, resolved. The
  /// executor is the exporter's to name (the declaration does not know it),
  /// so it is given here — `None` for a module that is linked, not loaded.
  fn header(executor: Option<Executor>) -> Header;

  /// The module as a store record, under the folder `parent`.
  fn record(parent: Uuid) -> frozen::Module;

  /// Every export's host closure, described.
  fn host_functions() -> Vec<HostFunction>;
}
