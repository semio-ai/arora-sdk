//! What a declared module and its boundary types implement. In the SDK these
//! belong in `arora-types`, beside `AroraType`.

use arora_types::call::{Call, CallError, CallResult};
use arora_types::module::low::Header;
use arora_types::record::module::frozen::Function;
use arora_types::record::Version;
use arora_types::Uuid;

/// One exported function as a host registers it: id, name, frozen signature,
/// and the closure that takes a `Call` (arguments by parameter id) to its
/// `CallResult`.
pub type HostFunction = (
  Uuid,
  &'static str,
  Function,
  Box<dyn FnMut(Call) -> Result<CallResult, CallError>>,
);

/// A declared Arora module — the twin of [`arora_types::AroraType`] for
/// modules. Implemented by the marker type a declaration emits (`polly::Module`).
pub trait AroraModule {
  /// The module's id.
  fn id() -> Uuid;
  /// The module's header: what a `module.yaml` declares, resolved. The
  /// executor is the exporter's to name (the declaration does not know it).
  fn header(executor: arora_types::module::low::Executor) -> Header;
  /// The module as a store record, under the folder `parent`.
  fn record(parent: Uuid) -> arora_types::record::module::frozen::Module;
  /// Every export's host closure, described.
  fn host_functions() -> Vec<HostFunction>;
}

/// The record version a boundary type is pinned at in a frozen signature —
/// `#[arora(version = "…")]`, `1.0.0` when absent. Belongs on `AroraType`.
pub trait AroraTypeVersion {
  fn arora_type_version() -> Version;
}
