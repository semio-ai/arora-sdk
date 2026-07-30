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
//! the store under the [`built_in`] keys before it ticks, so an interpreter that
//! needs `dt` or elapsed time reads it from the store like any other slot.

use arora_types::call::{Call, CallBridge};
use arora_types::data::DataStore;

pub mod built_in;
pub mod graph;
pub mod interpreter_module;
pub mod status;
pub mod task;

pub use graph::{Graph, GraphDiff};
pub use status::{
    declare_status_enumeration, Status, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION,
    STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID, STATUS_SUCCESS_VARIANT_ID,
};
pub use task::{RunPolicy, TaskHandle, TaskId};

/// Whether an interpreter wants to be ticked again — and, when it does not, how
/// it ended.
///
/// Liveness is one bit: [`Running`](Self::Running) or terminal
/// ([`Done`](Self::Done)). The terminal case carries a `Result` — `Ok(())` for a
/// clean stop, `Err` for a fault the run could not recover from.
///
/// This is the interpreter's *scheduling* status, not an application outcome. A
/// behavior does not *return* a value — it writes its outputs to store keys — so
/// the terminal `Ok` is `()`, never a payload. A run's success / failure / errno
/// is likewise an application value on its status key, not here: one [`tick`] may
/// drive many runs, so a single return cannot speak for each. What the runtime
/// must act on is only liveness and, at termination, whether the run stopped
/// cleanly or faulted.
///
/// [`tick`]: BehaviorInterpreter::tick
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorStatus {
    /// Not terminal: tick me again next step (a node graph; a tree still
    /// running).
    Running,
    /// Terminal: I reached a terminal state and the runtime drops me. `Ok(())`
    /// is a clean stop; `Err(_)` is a fault the run could not recover from — its
    /// reason is surfaced (like an `Err` from [`tick`](BehaviorInterpreter::tick))
    /// but, unlike a transient `Err`, a run that ends `Done(Err(_))` is dropped,
    /// *not* retried.
    Done(Result<(), BehaviorError>),
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
    /// or [`BehaviorStatus::Done`] to be dropped — `Done(Ok(()))` for a clean
    /// stop, `Done(Err(e))` for a terminal fault whose reason `e` is surfaced.
    ///
    /// An `Err` from `tick` is different from `Ok(Done(Err(..)))`: an `Err` is a
    /// *transient* failure — the runtime keeps the interpreter installed and
    /// ticks it again next step (a momentary lowering or call-bridge hiccup),
    /// only raising the reason on the standing error watch. `Done(Err(..))` is
    /// *terminal*: the run has ended and will not be retried.
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
    /// [`GraphDiff`](graph::GraphDiff) arrives — notably the interpreter's
    /// engine-registered edit function, which holds no store.
    fn apply(&mut self, diff: graph::GraphDiff) -> Result<(), BehaviorError> {
        let _ = diff;
        Err(BehaviorError {
            message: "this interpreter does not support graph edition".to_string(),
        })
    }

    /// Replace the running behavior with `graph`, whole. Like
    /// [`apply`](Self::apply) it is store-less — an implementation may defer
    /// lowering to the next [`tick`](Self::tick) — so it is callable from the
    /// interpreter's engine-registered load function. The default rejects
    /// loading, for hand-coded interpreters that carry no graph.
    fn load(&mut self, graph: graph::Graph) -> Result<(), BehaviorError> {
        let _ = graph;
        Err(BehaviorError {
            message: "this interpreter does not support loading a graph".to_string(),
        })
    }

    /// Spawn `call` as a concurrent task run under `policy`, returning a
    /// [`TaskHandle`] to follow and stop it. Non-blocking: the run advances with
    /// the normal [`tick`](Self::tick), and its lifecycle surfaces on the
    /// handle's keys.
    ///
    /// The default rejects spawning — override it in interpreters that host
    /// concurrent runs (a run is realised however the interpreter hosts
    /// behavior: a parallel behavior-tree subtree, a node-graph subgraph). A
    /// hand-coded interpreter can leave this as-is.
    fn spawn(&mut self, call: Call, policy: RunPolicy) -> Result<TaskHandle, BehaviorError> {
        let _ = (call, policy);
        Err(BehaviorError {
            message: "this interpreter does not support spawning task runs".to_string(),
        })
    }

    /// Halt the run identified by `task` (idempotent): a clean stop that may run
    /// compensation, not a hard kill. On reaching a terminal state the run is
    /// dropped and its status key reflects the outcome.
    ///
    /// The default rejects halting — override it alongside [`spawn`](Self::spawn).
    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        let _ = task;
        Err(BehaviorError {
            message: "this interpreter does not support halting task runs".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal interpreter that only ticks, so it keeps the default `spawn`
    /// and `halt` — the ones under test.
    struct Idle;
    impl BehaviorInterpreter for Idle {
        fn tick(&mut self, _ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
            Ok(BehaviorStatus::Running)
        }
    }

    #[test]
    fn spawn_and_halt_reject_by_default() {
        let mut idle = Idle;
        let call = Call {
            module_id: None,
            id: uuid::Uuid::nil(),
            args: Vec::new(),
        };
        assert!(idle.spawn(call, RunPolicy::Concurrent).is_err());
        assert!(idle.halt(TaskId(uuid::Uuid::nil())).is_err());
    }
}
