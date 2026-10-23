//! Arora's dataflow nodes: the node vocabulary as the functions of three
//! modules ([`functions`]), `@arora/dataflow`, `@arora/geometry` and
//! `@arora/kinematics`, each node kind one function, computing on Arora's
//! values.
//!
//! A loader of a graph format puts these calls on a behavior tree, each a
//! detached node, and [`functions::DataflowNodes`] answers them in-process.
//! What a loader needs is public: each kind's function and parameter ids
//! ([`functions`]), its ports ([`ports`]) and port layouts
//! ([`eval::plan`]), its parameters ([`params::Params`]) and where a tree
//! places it ([`functions::placement`]).

pub mod eval;
pub mod functions;
pub mod kind;
pub mod params;
pub mod ports;
pub mod select;
pub mod task;
pub mod vocab;

pub use eval::{Context, NodeFunction, NodeFunctionRegistry, NodeFunctions, SpawnRequest};
pub use kind::Kind;
pub use params::{Params, RoundMode};
pub use select::{Selector, SelectorSegment};
