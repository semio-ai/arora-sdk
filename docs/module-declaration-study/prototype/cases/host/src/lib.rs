//! The device side of the study. `host_module::<M>()` folds a declared
//! module's `host_functions()` into a `HostModule` — in the SDK, one function
//! on `arora_engine::module::ModuleBuilder`; `host_module!(path)` is the same
//! thing spelled as a macro.

use arora_engine::engine::{EngineBuilder, PinnedEngine};
use arora_engine::module::{HostModule, ModuleBuilder};
use arora_module_support::AroraModule;

/// A `HostModule` from any declared module.
pub fn host_module<M: AroraModule>() -> HostModule {
  let mut builder = ModuleBuilder::new(M::id());
  for (id, name, signature, function) in M::host_functions() {
    builder = builder.described_function(id, name, signature, function);
  }
  builder.build()
}

pub fn polly() -> HostModule {
  host_module::<case_polly::polly::Module>()
}

pub fn polly_outline() -> HostModule {
  arora_module_derive::host_module!(case_polly_outline::polly)
}

pub fn animation() -> HostModule {
  host_module::<case_animation::animation::Module>()
}

pub fn bt_nodes() -> HostModule {
  host_module::<case_bt_nodes::nodes::Module>()
}

/// An engine with the given modules registered — no executor, since nothing
/// is loaded from an artifact.
pub fn engine_with(modules: Vec<HostModule>) -> PinnedEngine {
  let mut engine = EngineBuilder::new().build();
  for module in modules {
    engine.register_module(module.id(), Box::new(module));
  }
  engine
}
