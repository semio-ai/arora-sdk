//! [`BehaviorTreeInterpreter`]: the [`BehaviorInterpreter`] that runs an Arora
//! behavior tree — an interpreter over the shared [`Graph`] model.
//!
//! # Task runs
//!
//! Beyond the one main tree, the interpreter also hosts **task runs**: the
//! concurrent, cancellable behaviors [`spawn`](BehaviorInterpreter::spawn)ed
//! through the interpreter-module ABI (a ROS 2 action maps onto one). A run is a
//! [`Call`] the interpreter invokes once per [`tick`](BehaviorInterpreter::tick),
//! alongside the main tree; the value the call returns is the run's
//! [`Status`], which the interpreter redirects to the run's own **status key**
//! (the "status decorator" the task-run design calls for). The run advances
//! until that status is terminal — or until it is
//! [`halt`](BehaviorInterpreter::halt)ed — and while any run is live the
//! interpreter stays installed to keep ticking it. Each run's keys live under
//! `arora/tasks/<module>/<function>/<run_id>/…`, so concurrent runs never
//! collide.

use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::graph::{Graph, GraphDiff};
use arora_behavior::{
    interpreter_module, BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus,
    RunPolicy, TaskHandle, TaskId,
};
use arora_types::call::Call;
use arora_types::data::{DataStore, Key, Slot, StateChange};
use arora_types::value::Value;
use uuid::Uuid;

use crate::arora_generated::behavior_tree::status::Status;
use crate::error::BehaviorTreeError;
use crate::graph::build_behavior_tree;
use crate::{run_behavior_tree, schema_groot, BehaviorTree, ModuleFunction};

/// One concurrent task run the interpreter hosts: the [`Call`] to invoke each
/// tick, the [`TaskHandle`] that names its keys, and whether a halt is pending.
struct Run {
    /// The call invoked once per tick; its returned value is the run's status.
    call: Call,
    /// The handle returned to the observer — its keys are where the run's
    /// lifecycle is published.
    handle: TaskHandle,
    /// Set by [`halt`](BehaviorInterpreter::halt); applied on the next tick
    /// (which owns the store), where the run ends and its status key is written.
    halt_requested: bool,
}

/// The [`BehaviorInterpreter`] that runs a [`BehaviorTree`].
///
/// It is an executor, not a behavior: construct it **empty and ready** with
/// [`new`](Self::new) — it holds only the module-function index it needs to
/// resolve call nodes, no tree — then load a behavior *into* it as a separate
/// step. It is never swapped.
///
/// With a tree loaded, each tick runs the tree to a terminal status
/// (success/failure), so it reports [`BehaviorStatus::Done`] — the run-once
/// semantics the engine's queued trees already had. With **no** tree loaded it
/// idles: every tick is a no-op reporting [`BehaviorStatus::Running`], so the
/// interpreter stays installed (it is never dropped) waiting for a behavior.
///
/// A behavior loaded from the shared [`Graph`] — [`load_graph`](Self::load_graph),
/// or Groot XML via [`load_groot`](Self::load_groot), which lowers onto the
/// graph — stays **editable**: [`apply`](BehaviorInterpreter::apply) mutates
/// the graph and re-lowers the tree against the context's store. A raw
/// [`BehaviorTree`] loaded with [`load`](Self::load) ticks fine but has no
/// authored graph, so `apply` is rejected.
pub struct BehaviorTreeInterpreter {
    tree: Option<BehaviorTree>,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    /// The authored graph behind the loaded tree, when there is one — what
    /// [`apply`](BehaviorInterpreter::apply) edits. The store the slots resolve
    /// against is never retained: lowering borrows it — at load from the
    /// caller, after an edit from the next tick's context.
    graph: Option<Graph>,
    /// An edit landed since the tree was last lowered; the next tick rebuilds
    /// before running.
    dirty: bool,
    /// The concurrent task runs spawned into this interpreter, keyed by id.
    /// Advanced each [`tick`](BehaviorInterpreter::tick) alongside the main
    /// tree; each publishes its lifecycle to its own status key. Empty until a
    /// [`spawn`](BehaviorInterpreter::spawn) lands.
    runs: HashMap<TaskId, Run>,
}

/// Bind a `{var}` name to the store slot under that name — the Direct
/// convention (variable name == store key).
fn direct_resolver(store: &dyn DataStore) -> impl Fn(&str) -> Option<Box<dyn Slot>> + '_ {
    move |name: &str| Some(store.slot(&Key::from(name)))
}

impl BehaviorTreeInterpreter {
    /// Construct an empty, ready interpreter over the module-function index its
    /// call nodes resolve against. It holds no behavior until one is loaded with
    /// [`load`](Self::load), [`load_graph`](Self::load_graph), or
    /// [`load_groot`](Self::load_groot); until then its tick idles.
    pub fn new(function_index: Rc<HashMap<Uuid, ModuleFunction>>) -> Self {
        Self {
            tree: None,
            function_index,
            graph: None,
            dirty: false,
            runs: HashMap::new(),
        }
    }

    /// Load an already-built [`BehaviorTree`] into the interpreter, replacing
    /// any behavior currently loaded. A raw tree carries no authored graph, so
    /// [`apply`](BehaviorInterpreter::apply) is rejected until a graph is
    /// loaded instead.
    pub fn load(&mut self, behavior: BehaviorTree) {
        self.tree = Some(behavior);
        self.graph = None;
        self.dirty = false;
    }

    /// Load a behavior from the shared [`Graph`], replacing any behavior
    /// currently loaded: lowers the graph to a runnable tree, binding its
    /// variables to `store`'s slots under their own names (the Direct
    /// convention). `store` must be the same store the device ticks against;
    /// it is only borrowed to resolve the slots — the tree keeps the slots,
    /// the interpreter keeps the graph (for edition), and nobody keeps the
    /// store.
    pub fn load_graph(
        &mut self,
        graph: Graph,
        store: &dyn DataStore,
    ) -> Result<(), BehaviorTreeError> {
        let tree = build_behavior_tree(&graph, &direct_resolver(store))?;
        self.tree = Some(tree);
        self.graph = Some(graph);
        self.dirty = false;
        Ok(())
    }

    /// Load a behavior tree from Groot XML, replacing any behavior currently
    /// loaded. The XML lowers onto the shared [`Graph`] (names → arora ids,
    /// `{var}` → named variables) and loads through
    /// [`load_graph`](Self::load_graph), so a Groot-loaded behavior is editable
    /// like any other graph.
    pub fn load_groot(
        &mut self,
        xml: &str,
        store: &dyn DataStore,
    ) -> Result<(), BehaviorTreeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)?;
        let graph = groot.into_graph(self.function_index.as_ref())?;
        self.load_graph(graph, store)
    }

    /// The authored graph, if the loaded behavior came from one.
    pub fn graph(&self) -> Option<&Graph> {
        self.graph.as_ref()
    }

    /// Advance every live task run one tick against `ctx`.
    ///
    /// Each run is invoked through the call bridge; the value it returns is its
    /// [`Status`], which is written to the run's status key — the "status
    /// decorator" that redirects a run's outcome onto its own key. A halted run
    /// is not invoked: it ends `Failure` (the engine has no "canceled" outcome
    /// of its own — the observer that issued the halt reports the cancel). A run
    /// that reaches a terminal status is dropped after this tick; its status (and
    /// any keys its call wrote) persist in the store for the observer to read.
    fn advance_runs(&mut self, ctx: &mut BehaviorContext) -> Result<(), BehaviorError> {
        if self.runs.is_empty() {
            return Ok(());
        }
        // Advance in a deterministic id order, so overlapping writes across
        // concurrent runs resolve reproducibly (last id wins under LWW).
        let mut ids: Vec<TaskId> = self.runs.keys().copied().collect();
        ids.sort_by_key(|id| id.0);

        let mut finished = Vec::new();
        for id in ids {
            let run = self.runs.get(&id).expect("id came from the run map");
            let status = if run.halt_requested {
                Status::Failure
            } else {
                let result = ctx
                    .call_bridge
                    .arora_call(run.call.clone())
                    .map_err(|e| BehaviorError {
                        message: format!("task run {}: {e:?}", id.0),
                    })?;
                // A run that returns a non-status value is treated as a clean
                // success — it ran and produced something, but nothing to keep
                // ticking for.
                Status::try_from(result.ret).unwrap_or(Status::Success)
            };
            let terminal = !matches!(status, Status::Running);
            let status_key = run.handle.status.clone();
            let status_value: Value = status.into();
            ctx.store
                .write(StateChange::set(status_key, status_value))
                .map_err(|e| BehaviorError {
                    message: format!("task run {}: writing status: {e}", id.0),
                })?;
            if terminal {
                finished.push(id);
            }
        }
        for id in finished {
            self.runs.remove(&id);
        }
        Ok(())
    }
}

impl BehaviorInterpreter for BehaviorTreeInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        // An edit landed since the last lowering: rebuild the tree from the
        // graph against this tick's store, so the edit (and any lowering
        // problem it introduced) takes effect here.
        if self.dirty {
            let graph = self.graph.as_ref().expect("dirty implies a graph");
            // Edits can leave the graph empty (or start it empty): nothing to
            // run, idle like a fresh interpreter instead of lowering nothing.
            self.tree = if graph.nodes.is_empty() {
                None
            } else {
                Some(
                    build_behavior_tree(graph, &direct_resolver(ctx.store)).map_err(|e| {
                        BehaviorError {
                            message: format!("rebuild after apply: {e:?}"),
                        }
                    })?,
                )
            };
            self.dirty = false;
        }
        // Run the loaded main tree to a terminal status, once (its run-once
        // semantics), if there is one. An idle interpreter (no tree) skips this.
        let ran_tree = if let Some(tree) = self.tree.as_ref() {
            run_behavior_tree(tree, self.function_index.clone(), ctx.call_bridge, false).map_err(
                |e| BehaviorError {
                    message: format!("behavior tree: {e:?}"),
                },
            )?;
            true
        } else {
            false
        };

        // Advance every concurrent task run against this tick's store and caller.
        self.advance_runs(ctx)?;

        // Liveness. While any run is live the interpreter must stay installed to
        // keep ticking it, so a main tree that ran (run-once) is dropped — it
        // will not re-run each tick — and we report `Running`. With no live runs
        // the original rule holds: a tree that ran is `Done` (dropped); an idle
        // interpreter keeps `Running`, waiting for a behavior.
        if !self.runs.is_empty() {
            if ran_tree {
                self.tree = None;
            }
            Ok(BehaviorStatus::Running)
        } else if ran_tree {
            Ok(BehaviorStatus::Done(Ok(())))
        } else {
            Ok(BehaviorStatus::Running)
        }
    }

    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        // A fresh, empty interpreter is editable from the start: loading a
        // behavior IS applying a diff onto an empty graph. Only a tree loaded
        // without a graph representation (legacy XML `load`) cannot be edited.
        if self.graph.is_none() && self.tree.is_none() {
            self.graph = Some(Graph::empty());
        }
        let graph = self.graph.as_mut().ok_or_else(|| BehaviorError {
            message: "the loaded behavior has no editable graph; load one with load_graph or \
                      load_groot to edit it"
                .to_string(),
        })?;
        graph.apply(diff).map_err(|e| BehaviorError {
            message: format!("graph diff: {e}"),
        })?;
        self.dirty = true;
        Ok(())
    }

    fn load(&mut self, graph: Graph) -> Result<(), BehaviorError> {
        // A whole-behavior replacement: the previous tree is gone now, the new
        // graph lowers at the next tick (like an edit).
        self.tree = None;
        self.graph = Some(graph);
        self.dirty = true;
        Ok(())
    }

    // NB: spawn/halt below register and cancel task runs; the runs advance in
    // `tick` (via `advance_runs`), which is where the store is available.
    fn spawn(&mut self, call: Call, policy: RunPolicy) -> Result<TaskHandle, BehaviorError> {
        // v1 runs every task concurrently (tick-by-tick, last-write-wins on any
        // shared actuation keys). `RunPolicy` reserves richer arbitration for
        // later; until it lands the policy is accepted and treated as
        // `Concurrent`.
        let _ = policy;
        let id = TaskId(Uuid::new_v4());
        let handle = run_handle(id, &call);
        self.runs.insert(
            id,
            Run {
                call,
                handle: handle.clone(),
                halt_requested: false,
            },
        );
        // The run is registered, not yet ticked: its first status write lands on
        // the next `tick`, which owns the store.
        Ok(handle)
    }

    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        // A clean stop, applied on the next tick (which owns the store). Halting
        // an unknown or already-finished run is a no-op success — `halt` is
        // idempotent.
        if let Some(run) = self.runs.get_mut(&task) {
            run.halt_requested = true;
        }
        Ok(())
    }
}

/// Assemble a run's [`TaskHandle`], allocating its per-run keys under
/// `arora/tasks/<module>/<function>/<run_id>/…` so concurrent runs — even of the
/// same function — never collide. `stop` is the [`HALT`] call an observer issues
/// to cancel the run (it routes back to [`BehaviorInterpreter::halt`]).
///
/// [`HALT`]: arora_behavior::interpreter_module::HALT
fn run_handle(id: TaskId, call: &Call) -> TaskHandle {
    let module = call
        .module_id
        .map(|m| m.to_string())
        .unwrap_or_else(|| "none".to_string());
    let prefix = format!("arora/tasks/{module}/{}/{}", call.id, id.0);
    TaskHandle {
        id,
        stop: interpreter_module::encode_halt(id),
        status: Key::from(format!("{prefix}/status")),
        feedback: vec![Key::from(format!("{prefix}/feedback"))],
        result: vec![Key::from(format!("{prefix}/result"))],
        update: vec![Key::from(format!("{prefix}/update"))],
    }
}
