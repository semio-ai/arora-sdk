//! The per-node code the module's functions run: what each node kind
//! computes from its inputs, parameters, clock and state.
//!
//! - [`mod@eval_node`] dispatches a node to the code of its
//!   [`NodeType`](crate::types::NodeType).
//! - `graph_runtime` holds what a node's evaluation reads and writes besides
//!   its ports: the clock, its state, the inputs staged for it and the writes
//!   and runs it makes.
//! - `plan` lays out each node's ports.
//! - `value_layout` flattens structured values for numeric operators.
//! - `shape_helpers` validates declared output shapes and applies selectors.
//! - `numeric` and `variadic` provide shared math helpers.
//! - `urdfik` is gated behind the `urdf_ik` feature and packages the IK solver helpers.
//! - `vizij` holds what belongs to Vizij's graph format: the `input` node,
//!   declared output shapes and a keyed batch's writes.

pub mod eval_node;
pub(crate) mod graph_runtime;
mod node_function;
mod noise;
mod numeric;
pub(crate) mod plan;
pub(crate) mod shape_helpers;
#[cfg(feature = "urdf_ik")]
pub(crate) mod urdfik;
pub(crate) mod value_layout;
pub(crate) mod variadic;
pub(crate) mod vizij;

pub use graph_runtime::{GraphRuntime, SpawnRequest};
pub use node_function::{NodeFunction, NodeFunctionRegistry, NodeFunctions};
pub use value_layout::PortValue;
