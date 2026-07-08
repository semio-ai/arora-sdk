//! The behavior abstraction: anything the Arora runtime can tick.
//!
//! A [`Behavior`] is the runtime's "what to do each step". The behavior tree is
//! one (`arora-behavior-tree`'s `TreeBehavior`); a Vizij node graph is another.
//! The runtime accepts any of them interchangeably — it just swaps the
//! interpreter.
//!
//! Each tick a behavior gets a [`BehaviorContext`]: the shared
//! [`DataStore`](arora_types::data::DataStore) (read inputs, write intent /
//! outputs) and a [`CallBridge`](arora_types::call::CallBridge) (so module-calling
//! behaviors like the behavior tree can reach the engine). A behavior uses
//! whichever it needs — a graph reads/writes the store; the tree drives the
//! caller.
//!
//! Timing is not a tick argument. The runtime publishes the frame's clock into
//! the store under the [`golden`] keys before it ticks, so a behavior that needs
//! `dt` or elapsed time reads it from the store like any other slot.

use arora_types::call::CallBridge;
use arora_types::data::DataStore;

pub mod golden;

/// Whether a behavior wants to be ticked again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorStatus {
    /// Tick me again next step (a node graph; a tree still running).
    Running,
    /// I reached a terminal state; the runtime may drop me.
    Done,
}

/// What a [`Behavior`] receives each [`tick`](Behavior::tick).
pub struct BehaviorContext<'a> {
    /// The shared blackboard: read inputs, write intent / outputs.
    pub store: &'a dyn DataStore,
    /// The module-call bridge (the engine), for behaviors that call modules.
    pub caller: &'a mut dyn CallBridge,
}

/// A behavior failed to tick.
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

/// Something the Arora runtime ticks once per step.
///
/// Implemented by the behavior tree (`arora-behavior-tree`'s `TreeBehavior`) and
/// by external interpreters such as a Vizij node graph. The runtime holds a
/// queue of `Box<dyn Behavior>` and ticks them without knowing which is which.
///
/// Not `Send`: the runtime is a single-owner, single-thread step loop.
pub trait Behavior {
    /// Advance one step. Return [`BehaviorStatus::Running`] to be ticked again,
    /// or [`BehaviorStatus::Done`] to be dropped.
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError>;
}
