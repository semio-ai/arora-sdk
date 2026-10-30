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
//! The runtime holds one `Box<dyn BehaviorInterpreter>`, set when the device is
//! built, and ticks it each step; a behavior is loaded into it, and edited in
//! place, through the interpreter. The behavior tree is one interpreter
//! (`arora-behavior-tree`'s [`BehaviorTreeInterpreter`]); a Vizij node graph is
//! another. Adding a new *kind of authored behavior* means adding a new
//! interpreter here. Hand-implementing the trait to hard-code a single behavior
//! in Rust is possible, but it is a corner case — the promoted path is to author
//! a behavior in an editor and let an interpreter run it.
//!
//! An authored behavior is a [`graph::Graph`] — nodes bound to functions, each
//! holding its ports' bindings — and interpreters are edited by
//! [`apply`](BehaviorInterpreter::apply)ing a [`graph::GraphDiff`]. Loading a
//! behavior is applying a diff onto an empty graph. See [`graph`] for the model.
//! [`read`](BehaviorInterpreter::read) answers the behavior as a client loads
//! it, with its revision, which counts the loads and edits.
//!
//! Each tick an interpreter gets a [`BehaviorContext`]: the shared
//! [`DataStore`](arora_types::data::DataStore) (read inputs, write intent /
//! outputs), a [`CallBridge`](arora_types::call::CallBridge) (so a
//! module-calling interpreter like the behavior tree can reach the engine),
//! and the device's [`FunctionIndex`] (the module functions it can call, with
//! their signatures). An interpreter uses whichever it needs — a graph
//! reads/writes the store; the tree drives the caller.
//!
//! Timing is not a tick argument. The runtime publishes the frame's clock into
//! the store under the [`built_in`] keys before it ticks, so an interpreter that
//! needs `dt` or elapsed time reads it from the store like any other key.

use std::collections::HashMap;

use arora_types::call::{Call, CallBridge};
use arora_types::data::DataStore;
use arora_types::record::module::frozen;
use arora_types::Uuid;
use serde::{Deserialize, Serialize};

pub mod built_in;
pub mod graph;
pub mod interpreter_module;
pub mod status;
pub mod task;
pub mod trace;

pub use graph::{Graph, GraphDiff, GraphType};
pub use status::{
    declare_status_enumeration, Status, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION,
    STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID, STATUS_SUCCESS_VARIANT_ID,
};
pub use task::{RunPolicy, TaskHandle, TaskId};
pub use trace::{Activity, NodeStatus, PortValue, TraceOptions, TraceReport};

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

/// The main behavior an interpreter runs, as a client loads it, and its
/// revision: what [`BehaviorInterpreter::read`] answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadedBehavior {
    /// The behavior's revision: 0 before anything is loaded, then one more at
    /// each [`load`](BehaviorInterpreter::load) and at each
    /// [`apply`](BehaviorInterpreter::apply) that changes the main behavior.
    /// Task runs started, edited or halted leave it as it is.
    pub revision: u64,
    /// The main behavior: the graph a [`load`](BehaviorInterpreter::load)
    /// of it reproduces, with no node of the interpreter's own and none of a
    /// task run's, rooted where it was loaded or last edited to be.
    /// [`Graph::empty`](graph::Graph::empty) when nothing is loaded.
    pub graph: graph::Graph,
}

/// One module function a device can call: the module that serves it, its
/// name, and its frozen signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFunction {
    /// The module a call to the function names.
    pub module_id: Uuid,
    /// The function's id.
    pub function_id: Uuid,
    /// The name it is described and found by.
    pub function_name: String,
    /// Its parameters (name, type, order) and return type.
    pub function: frozen::Function,
}

/// The module functions a device can call, by function id: its method index.
///
/// The device owns it and changes it when its modules change — a module
/// loaded, reloaded or unloaded between two steps — and hands it to its
/// interpreter at each [`tick`](BehaviorInterpreter::tick), through
/// [`BehaviorContext::functions`]. An interpreter reads it when it resolves
/// what its behavior calls.
pub type FunctionIndex = HashMap<Uuid, ModuleFunction>;

/// What a [`BehaviorInterpreter`] receives each
/// [`tick`](BehaviorInterpreter::tick).
pub struct BehaviorContext<'a> {
    /// The shared blackboard: read inputs, write intent / outputs.
    pub store: &'a dyn DataStore,
    /// The module-call bridge (the engine), for interpreters that call modules.
    pub call_bridge: &'a mut dyn CallBridge,
    /// The module functions the device can call this tick, with their
    /// signatures.
    pub functions: &'a FunctionIndex,
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
    /// set or remove whole nodes, bind or unbind single ports, set the root.
    /// **Loading** a behavior is applying a diff onto an empty graph.
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

    /// The main behavior and its revision ([`LoadedBehavior`]). The graph is
    /// what a client [`load`](Self::load)s to run the same behavior: none of
    /// the interpreter's own structure, none of a task run's nodes, the root
    /// as loaded or as the last edit set it. Reading changes nothing.
    ///
    /// An interpreter that answers it keeps the revision: one more at each
    /// `load`, and at each `apply` that changes the main behavior — not at a
    /// refused one, nor at a spawn, a halt, an edit of a task run's nodes or a
    /// tick. It writes the revision to
    /// [`built_in::BEHAVIOR_REVISION`] at its first tick and at the tick
    /// after each change, and each of its traces' reports carries it when it
    /// changed since that trace's previous report
    /// ([`TraceReport::revision`](trace::TraceReport::revision)). A client
    /// that follows either learns that another client changed the behavior,
    /// and reads it again.
    ///
    /// The default rejects reading — override it in interpreters that carry
    /// a loadable [`graph::Graph`] (the behavior tree does).
    fn read(&self) -> Result<LoadedBehavior, BehaviorError> {
        Err(BehaviorError {
            message: "this interpreter does not support reading its behavior".to_string(),
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

    /// Spawn `graph`, written in the language `graph_type` names, as a task
    /// run under `policy`, returning a [`TaskHandle`] to follow and stop it.
    /// The graph is the run's program: it runs beside the main behavior,
    /// ticked with it each step, until it reaches a terminal state or is
    /// [`halt`](Self::halt)ed. Non-blocking, like [`spawn`](Self::spawn), and
    /// its lifecycle surfaces on the handle's keys.
    ///
    /// An interpreter refuses a `graph_type` it does not read — another
    /// language, another major version, or a newer minor version than it
    /// reads (see [`GraphType::reads`]) — with an error naming the type it
    /// reads.
    ///
    /// The run's nodes are part of the interpreter's graph: an
    /// [`apply`](Self::apply) reaches them by id, so a client edits a live run
    /// (binds a port to another value, replaces a node) without respawning
    /// it, and a [`load`](Self::load) of the main behavior leaves them in
    /// place.
    ///
    /// A run is known by its graph's root, which the client chooses. Spawning
    /// a graph whose root a live run has starts no second run: that run takes
    /// the graph as its program, keeping its id, status key and policy, and
    /// the answer is its handle. So a client that lost a run's [`TaskId`]
    /// takes the run over by spawning its graph again, a retried spawn does
    /// not start a second copy, and the handle answered always names the run
    /// that runs the graph spawned. Every client holding that handle halts the
    /// one run.
    ///
    /// The default rejects spawning a graph — override it in interpreters that
    /// host a graph as a run (a behavior-tree subtree, a node-graph subgraph).
    fn spawn_graph(
        &mut self,
        graph_type: &GraphType,
        graph: graph::Graph,
        policy: RunPolicy,
    ) -> Result<TaskHandle, BehaviorError> {
        let _ = (graph_type, graph, policy);
        Err(BehaviorError {
            message: "this interpreter does not support spawning a graph".to_string(),
        })
    }

    /// Start a trace of the running behavior under `options`, returning the
    /// [`TaskHandle`] a spawned run answers: a trace is a task run, followed
    /// and stopped like one. Its keys live under
    /// `arora/tasks/<interpreter module>/<trace function>/<run>/`
    /// ([`interpreter_module::ID`], [`interpreter_module::TRACE`]).
    ///
    /// While the trace runs, its status key holds
    /// [`Status::Running`], and at the end of each tick that changed
    /// something it follows, its feedback key holds a
    /// [`TraceReport`](trace::TraceReport): the tick, the nodes whose status
    /// changed, the detached nodes whose activity changed, and the watched
    /// nodes' outputs that changed. The first report states everything the
    /// tick reached. [`halt`](Self::halt) ends a trace: its status key goes
    /// [`Status::Failure`] at the next tick, as a halted run's does, and its
    /// keys change no more. Several traces may run at once, each with its own
    /// options; a trace survives [`apply`](Self::apply) and
    /// [`load`](Self::load).
    ///
    /// The default rejects tracing — override it in interpreters that can
    /// report what their ticks do.
    fn trace(&mut self, options: trace::TraceOptions) -> Result<TaskHandle, BehaviorError> {
        let _ = options;
        Err(BehaviorError {
            message: "this interpreter does not support tracing".to_string(),
        })
    }

    /// Halt the run identified by `task` (idempotent): a clean stop that may run
    /// compensation, not a hard kill. On reaching a terminal state the run is
    /// dropped and its status key reflects the outcome. It stops a run started
    /// by [`spawn`](Self::spawn), by [`spawn_graph`](Self::spawn_graph) or by
    /// [`trace`](Self::trace).
    ///
    /// The default rejects halting — override it alongside [`spawn`](Self::spawn).
    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        let _ = task;
        Err(BehaviorError {
            message: "this interpreter does not support halting task runs".to_string(),
        })
    }

    /// The methods this interpreter implements itself, by function id, each
    /// with its name and frozen signature: task runs it hosts as behavior (a
    /// node-graph fragment, a behavior-tree subtree) rather than as a module
    /// call. The runtime lists them in the device's method index under the
    /// interpreter module ([`interpreter_module::ID`]), so a remote discovers
    /// one over method introspection and [`spawn`](Self::spawn)s it like any
    /// task run.
    ///
    /// Asked once, when the runtime is built. The default describes none.
    fn described_methods(&self) -> HashMap<Uuid, frozen::Export> {
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal interpreter that only ticks, so it keeps the default `read`,
    /// `spawn`, `spawn_graph`, `trace` and `halt` — the ones under test.
    struct Idle;
    impl BehaviorInterpreter for Idle {
        fn tick(&mut self, _ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
            Ok(BehaviorStatus::Running)
        }
    }

    #[test]
    fn an_interpreter_describes_no_method_by_default() {
        assert!(Idle.described_methods().is_empty());
    }

    #[test]
    fn spawning_and_halting_reject_by_default() {
        let mut idle = Idle;
        let call = Call {
            module_id: None,
            id: uuid::Uuid::nil(),
            args: Vec::new(),
        };
        assert!(idle.spawn(call, RunPolicy::Concurrent).is_err());
        assert!(idle
            .spawn_graph(
                &GraphType {
                    name: "behavior-tree".to_string(),
                    version: semver::Version::new(1, 0, 0),
                },
                graph::Graph::empty(),
                RunPolicy::Concurrent
            )
            .is_err());
        assert!(idle.halt(TaskId(uuid::Uuid::nil())).is_err());
    }

    #[test]
    fn reading_rejects_by_default() {
        let refused = Idle.read().unwrap_err();
        assert!(refused.message.contains("reading"), "{}", refused.message);
    }

    #[test]
    fn tracing_rejects_by_default() {
        let refused = Idle.trace(TraceOptions::default()).unwrap_err();
        assert!(refused.message.contains("tracing"), "{}", refused.message);
    }
}
