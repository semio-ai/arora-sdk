//! Arora's dataflow nodes as host modules of the device
//! ([`AroraBuilder::with_dataflow_nodes`](crate::AroraBuilder::with_dataflow_nodes)):
//! the arithmetic, logic, comparisons, vectors, smoothing and records of
//! [`arora_dataflow_nodes`], which a tree calls in a node's arguments
//! (`is_true(value: greaterthan(lhs: {top}, rhs: 0.006f32))`) or as a leaf.
//!
//! Each module of [`function_index`] (`@arora/dataflow`, `@arora/geometry`,
//! and `@arora/kinematics` when built in) is a host module describing its
//! functions with their signatures, so the device's method index, which a
//! Groot tree's tags and a text's names resolve against, has them. One
//! [`DataflowNodes`] answers their calls, in process.
//!
//! What a dataflow function would call in another module (`external`,
//! `taskrun`, `spawn`) is refused: a host module's function is handed its
//! call alone, not the device's bridge.
//!
//! A tree from text or Groot passes no `node` argument, so its calls share
//! one call site in [`DataflowNodes`], and what it keeps does not grow as
//! behaviors are loaded; nothing calls `retain_sites`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use arora_dataflow_nodes::functions::{function_index, DataflowNodes};
use arora_engine::module::{HostModule, ModuleBuilder};
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::value::Value;

/// The host modules serving the dataflow nodes, one per module id, sharing
/// one [`DataflowNodes`].
pub(crate) fn modules() -> Vec<HostModule> {
    let nodes = Rc::new(RefCell::new(DataflowNodes::new()));
    let mut by_module = BTreeMap::new();
    for function in function_index().values() {
        by_module
            .entry(function.module_id)
            .or_insert_with(Vec::new)
            .push(function);
    }
    by_module
        .into_iter()
        .map(|(module_id, mut functions)| {
            functions.sort_by_key(|function| function.function_id);
            let mut builder = ModuleBuilder::new(module_id);
            for function in functions {
                let nodes = nodes.clone();
                builder = builder.described_function(
                    function.function_id,
                    function.function_name.clone(),
                    function.function.clone(),
                    move |call| nodes.borrow_mut().call(call, &mut NoModules),
                );
            }
            builder.build()
        })
        .collect()
}

/// The bridge a dataflow function's own calls to other modules meet: none is
/// served.
struct NoModules;

impl CallBridge for NoModules {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        Err(CallError::Generic {
            message: format!(
                "a dataflow node on this device calls no other module ({})",
                call.id
            ),
        })
    }

    fn arora_register_callable(&mut self, _: Rc<dyn Callable>) -> CallableId {
        CallableId { id: u64::MAX }
    }

    fn arora_unregister_callable(&mut self, _: &CallableId) {}

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        Err(CallError::Internal {
            message: format!("no callable {}", callable_id.id),
        })
    }
}
