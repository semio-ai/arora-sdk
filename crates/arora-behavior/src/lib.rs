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
//! The runtime holds a queue of `Box<dyn BehaviorInterpreter>` and ticks them
//! interchangeably — it just swaps the interpreter. The behavior tree is one
//! interpreter (`arora-behavior-tree`'s [`BehaviorTreeInterpreter`]); a Vizij
//! node graph is another. Adding a new *kind of authored behavior* means adding
//! a new interpreter here. Hand-implementing the trait to hard-code a single
//! behavior in Rust is possible, but it is a corner case — the promoted path is
//! to author a behavior in an editor and let an interpreter run it.
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
    pub caller: &'a mut dyn CallBridge,
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
/// node-graph interpreter. The runtime holds a queue of
/// `Box<dyn BehaviorInterpreter>` and ticks them without knowing which is which.
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
}
