//! Node graphs on Arora's behavior tree.
//!
//! This crate defines the graph schema, the node registry metadata and topology helpers, the
//! node vocabulary as the functions of Arora modules ([`functions`]), Vizij's graph format's own
//! functions ([`vizij_functions`]), the importer from a graph to a behavior tree ([`import`]), and
//! [`TreeRuntime`](tree_runtime::TreeRuntime), which runs a graph on the tree.

pub mod eval;
pub mod functions;
pub mod import;
pub mod kind;
pub mod params;
pub mod ports;
pub mod schema;
pub mod select;
pub mod task;
pub mod topo;
pub mod tree_runtime;
pub mod types;
pub mod vizij_functions;
pub mod vocab;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tree_tests;

pub use eval::{Context, NodeFunction, NodeFunctionRegistry, NodeFunctions, PortValue};
pub use schema::registry;
pub use topo::topo_order;
pub use types::*;
