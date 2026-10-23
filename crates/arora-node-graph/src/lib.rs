//! Node graphs on Arora's behavior tree.
//!
//! This crate defines the graph schema, the node registry metadata and topology helpers, the
//! node vocabulary as the functions of an Arora module ([`functions`]), the importer from a graph
//! to a behavior tree ([`import`]), and [`TreeRuntime`](tree_runtime::TreeRuntime), which runs a
//! graph on the tree.

pub mod eval;
pub mod functions;
pub mod import;
pub mod schema;
pub mod task;
pub mod topo;
pub mod tree_runtime;
pub mod types;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tree_tests;

pub use eval::{
    GraphRuntime, NodeFunction, NodeFunctionRegistry, NodeFunctions, PortValue, StagedInput,
};
pub use schema::registry;
pub use topo::topo_order;
pub use types::*;
