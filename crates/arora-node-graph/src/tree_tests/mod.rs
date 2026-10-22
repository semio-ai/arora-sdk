//! The node graph's tests, run on the behavior tree: each file here is the
//! counterpart of the reference evaluator's test file of the same name in
//! [`crate::eval`], with the graph evaluated by a [`TreeRuntime`] where the
//! reference tests use a [`GraphRuntime`](crate::GraphRuntime).
//!
//! [`TreeRuntime`]: crate::tree_runtime::TreeRuntime
mod equivalence;
