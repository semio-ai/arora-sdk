//! The per-node code the module's functions run: what each node kind
//! computes from its inputs, parameters, clock and state.
//!
//! - [`mod@eval_node`] dispatches a node to the code of its
//!   [`Kind`](crate::kind::Kind).
//! - `context` holds what a node's evaluation reads and writes besides its
//!   ports: the clock, its state and the runs it requests.
//! - [`plan`] lays out each node's ports.
//! - [`value_layout`] flattens structured values for numeric operators.
//! - `numeric` and [`variadic`] provide shared math helpers.
//! - `urdfik` is gated behind the `urdf_ik` feature and packages the IK solver helpers.

pub mod context;
pub mod eval_node;
mod node_function;
mod noise;
mod numeric;
pub mod plan;
#[cfg(feature = "urdf_ik")]
pub(crate) mod urdfik;
pub mod value_layout;
pub mod variadic;

pub use context::{Context, SpawnRequest};
pub use node_function::{NodeFunction, NodeFunctionRegistry, NodeFunctions};
