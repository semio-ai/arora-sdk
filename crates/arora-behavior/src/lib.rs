//! Behavior interpreters: the executors the Arora runtime ticks each step.
//!
//! Two things share the word "behavior", and this crate is about only one of
//! them:
//!
//! - A **behavior** (the noun) is an *authored, editable representation* of what
//!   a device should do — a behavior tree, a node graph — produced in a visual
//!   editor (Studio, the Vizij Workspace) and shipped as data.
//! - A [`BehaviorInterpreter`] is the *runtime-level executor* that runs one of
//!   those: a behavior-tree interpreter, a node-graph interpreter. It is the
//!   thing the runtime actually ticks.
//!
//! The runtime holds one `Box<dyn BehaviorInterpreter>` and ticks it — swapping
//! the interpreter replaces the behavior. The behavior tree is one interpreter
//! (`arora-behavior-tree`'s [`BehaviorTreeInterpreter`]); a Vizij node graph is
//! another. Adding a new *kind of authored behavior* means adding a new
//! interpreter here. Hand-implementing the trait to hard-code a single behavior
//! in Rust is possible, but it is a corner case — the promoted path is to author
//! a behavior in an editor and let an interpreter run it.
//!
//! An authored behavior is a [`graph::Graph`] — nodes bound to functions, with
//! typed I/Os and links — and interpreters are edited by
//! [`apply`](BehaviorInterpreter::apply)ing a [`graph::GraphDiff`]. Loading a
//! behavior is applying a diff onto an empty graph. See [`graph`] for the model.
//!
//! Each tick an interpreter gets a [`BehaviorContext`]: the shared
//! [`DataStore`](arora_types::data::DataStore) (read inputs, write intent /
//! outputs) and a [`CallBridge`](arora_types::call::CallBridge) (so a
//! module-calling interpreter like the behavior tree can reach the engine). An
//! interpreter uses whichever it needs — a graph reads/writes the store; the
//! tree drives the caller.
//!
//! Timing is not a tick argument. The runtime publishes the frame's clock into
//! the store under the [`golden`] keys before it ticks, so an interpreter that
//! needs `dt` or elapsed time reads it from the store like any other slot.

use arora_types::call::CallBridge;
use arora_types::data::DataStore;

pub mod golden;
pub mod graph;

pub use graph::{Graph, GraphDiff};

/// Whether an interpreter wants to be ticked again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorStatus {
    /// Tick me again next step (a node graph; a tree still running).
    Running,
    /// I reached a terminal state; the runtime may drop me.
    Done,
}

/// What a [`BehaviorInterpreter`] receives each
/// [`tick`](BehaviorInterpreter::tick).
pub struct BehaviorContext<'a> {
    /// The shared blackboard: read inputs, write intent / outputs.
    pub store: &'a dyn DataStore,
    /// The module-call bridge (the engine), for interpreters that call modules.
    pub call_bridge: &'a mut dyn CallBridge,
}

/// An interpreter failed to tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorError {
    /// Human-readable description.
    pub message: String,
}

impl std::fmt::Display for BehaviorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BehaviorError {}

/// A behavior *executor*: something the Arora runtime ticks once per step to run
/// an authored behavior.
///
/// Implemented by the behavior tree (`arora-behavior-tree`'s
/// [`BehaviorTreeInterpreter`]) and by other interpreters such as a Vizij
/// node-graph interpreter. The runtime holds one `Box<dyn BehaviorInterpreter>`
/// and ticks it without knowing which is which.
///
/// Implement this to add a new *kind of executor* (a new authored-behavior
/// representation the runtime can run), not to hand-code one particular
/// behavior — authored behaviors come from the visual editors, run by an
/// existing interpreter.
///
/// Not `Send`: the runtime is a single-owner, single-thread step loop.
pub trait BehaviorInterpreter {
    /// Advance one step. Return [`BehaviorStatus::Running`] to be ticked again,
    /// or [`BehaviorStatus::Done`] to be dropped.
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError>;

    /// Edit the running behavior by applying a [`GraphDiff`](graph::GraphDiff):
    /// add/remove nodes and links, set/override predetermined keys. **Loading** a
    /// behavior is applying a diff onto an empty graph.
    ///
    /// The default rejects edition — override it in interpreters that carry an
    /// editable [`graph::Graph`] (the behavior tree does). A hand-coded,
    /// non-graph interpreter (the corner case above) can leave this as-is.
    ///
    /// `apply` validates and stores the edit; an implementation may defer
    /// re-lowering its runtime form to the next [`tick`](Self::tick) (which
    /// carries the store in its context), so a lowering problem can surface
    /// there rather than here. This keeps `apply` callable from anywhere a
    /// [`GraphDiff`](graph::GraphDiff) arrives — notably an engine-registered
    /// edit function, which holds no store.
    fn apply(&mut self, diff: graph::GraphDiff) -> Result<(), BehaviorError> {
        let _ = diff;
        Err(BehaviorError {
            message: "this interpreter does not support graph edition".to_string(),
        })
    }
}
