//! [`BehaviorTreeInterpreter`]: the [`BehaviorInterpreter`] that runs an Arora
//! behavior tree — an interpreter over the shared [`Graph`] model.
//!
//! # The runner scaffold
//!
//! The interpreter permanently hosts one graph: a **runner** — a parallel root —
//! under which everything it executes is grafted as tree structure:
//!
//! - [`load`](BehaviorInterpreter::load)ing a behavior grafts it as the
//!   runner's *main* child (replacing the previous one);
//! - [`spawn`](BehaviorInterpreter::spawn)ing a task run grafts a two-node
//!   **fragment** — a run-status decorator over a run-call leaf carrying the
//!   spawned [`Call`] as literal data — beside it;
//! - [`halt`](BehaviorInterpreter::halt)ing prunes the fragment.
//!
//! Everything is ordinary graph data, edited through the same
//! [`apply`](BehaviorInterpreter::apply)/[`GraphDiff`] path an editor uses and
//! visible through [`graph`](BehaviorTreeInterpreter::graph) — the runs, their
//! policies, and the loaded behavior are introspectable *the behavior tree's
//! way*, not hidden interpreter state. Concurrency is the parallel runner's
//! ordinary semantics; richer [`RunPolicy`] arbitration lands as runner-node
//! kinds and decorators, still as visible structure.
//!
//! # Tick semantics
//!
//! **One arora runtime tick is one behavior-tree tick.** Each
//! [`tick`](BehaviorInterpreter::tick) ticks the runner once: instantaneous
//! subtrees drill through in a single pass, and `Running` means "see you next
//! tick" — a run (or a `run` builtin) waiting on outside state simply stays
//! `Running` across ticks while the device loop goes on. A terminal main
//! behavior is re-evaluated on the next tick (a behavior tree is a continuously
//! re-evaluated policy); a terminal *run* latches — its status key holds the
//! outcome, the fragment sits inert until pruned at the next graph edit.
//!
//! The lowered tree persists across ticks and is rebuilt only when the graph
//! changed (an edit, a load, a spawn, a prune), unregistering the previous
//! lowering's engine callables. Re-lowering resets node-parameter state (a
//! `seq_star` index, a run's latch) — which is why finished fragments are
//! pruned before the next lowering rather than left to re-fire.

use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::graph::{Graph, GraphDiff, Io, Link, LinkSource, Node as GraphNode, Port};
use arora_behavior::{
    interpreter_module, BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus,
    RunPolicy, TaskHandle, TaskId,
};
use arora_types::call::Call;
use arora_types::data::{DataStore, Key, Slot, StateChange};
use arora_types::value::Value;
use arora_types::value_serde;
use uuid::Uuid;

use crate::arora_generated::behavior_tree::status::Status;
use crate::graph::build_behavior_tree;
use crate::nodes::{
    PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID, RUN_CALL_PARAM_ID, RUN_STATUS_FUNCTION_ID,
    RUN_STATUS_LATCH_PARAM_ID, RUN_STATUS_OUT_PARAM_ID,
};
use crate::{lower_behavior_tree, schema_groot, LoweredTree, ModuleFunction};

/// One live task run's place in the scaffold: its two fragment nodes and the
/// status key its decorator publishes to. The graph holds the truth; this is
/// the bookkeeping that maps a [`TaskId`] onto it.
struct Run {
    decorator: Uuid,
    call_node: Uuid,
    status_key: Key,
}

/// The [`BehaviorInterpreter`] that runs a [`Graph`] as a behavior tree.
///
/// It is an executor, not a behavior: construct it **empty and ready** with
/// [`new`](Self::new) — it hosts the runner scaffold from the start — then load
/// a behavior *into* it ([`load`](BehaviorInterpreter::load), or Groot XML via
/// [`load_groot`](Self::load_groot)) and spawn task runs beside it. See the
/// [module docs](self) for the scaffold and the tick semantics.
pub struct BehaviorTreeInterpreter {
    /// The scaffold graph: the runner root, the main behavior, the fragments.
    graph: Graph,
    /// The runner (parallel root) node id.
    runner: Uuid,
    /// The loaded main behavior: its node ids (replaced wholesale by a load)
    /// and its root (the runner's first child).
    main: Vec<Uuid>,
    main_root: Option<Uuid>,
    /// Live task runs, by id.
    runs: HashMap<TaskId, Run>,
    /// Finished fragments awaiting pruning: latched inert, removed at the next
    /// graph edit (a re-lowering would reset their latches and re-fire them).
    finished: Vec<(Uuid, Uuid)>,
    /// Halts requested since the last tick; the next tick (which owns the
    /// store) writes their terminal status and prunes them.
    pending_halts: Vec<TaskId>,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    /// The persistent lowering; rebuilt when [`dirty`](Self::dirty).
    lowered: Option<LoweredTree>,
    /// The graph changed since the last lowering.
    dirty: bool,
}

/// Bind a `{var}` name to the store slot under that name — the Direct
/// convention (variable name == store key).
fn direct_resolver(store: &dyn DataStore) -> impl Fn(&str) -> Option<Box<dyn Slot>> + '_ {
    move |name: &str| Some(store.slot(&Key::from(name)))
}

impl BehaviorTreeInterpreter {
    /// Construct the interpreter over the module-function index its call nodes
    /// resolve against. It hosts the runner scaffold immediately; with nothing
    /// loaded the runner ticks no children and the interpreter idles.
    pub fn new(function_index: Rc<HashMap<Uuid, ModuleFunction>>) -> Self {
        let runner = Uuid::new_v4();
        let mut graph = Graph::empty();
        graph.nodes.insert(
            runner,
            GraphNode {
                id: runner,
                function: PARALLEL_FUNCTION_ID,
                children: Some(Vec::new()),
                ..GraphNode::default()
            },
        );
        graph.root = Some(runner);
        Self {
            graph,
            runner,
            main: Vec::new(),
            main_root: None,
            runs: HashMap::new(),
            finished: Vec::new(),
            pending_halts: Vec::new(),
            function_index,
            lowered: None,
            dirty: true,
        }
    }

    /// Load a behavior tree from Groot XML, replacing the main behavior. The
    /// XML lowers onto the shared [`Graph`] (names → arora ids, `{var}` →
    /// named variables) and grafts through [`load`](BehaviorInterpreter::load),
    /// so a Groot-loaded behavior is editable and introspectable like any
    /// other.
    pub fn load_groot(&mut self, xml: &str) -> Result<(), crate::error::BehaviorTreeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)?;
        let graph = groot.into_graph(self.function_index.as_ref())?;
        self.load(graph)
            .map_err(|e| crate::error::BehaviorTreeError::InconsistentTreeError {
                message: e.to_string(),
            })
    }

    /// The scaffold graph: the runner, the loaded behavior, and every task-run
    /// fragment — the interpreter's whole live structure, as data.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The runner node with its children recomputed from the current state:
    /// the main behavior's root first, then every live and finished fragment's
    /// decorator. Re-adding it (same id) in a diff replaces it.
    fn runner_node(&self) -> GraphNode {
        let mut children = Vec::with_capacity(1 + self.runs.len() + self.finished.len());
        children.extend(self.main_root);
        children.extend(self.runs.values().map(|run| run.decorator));
        children.extend(self.finished.iter().map(|(decorator, _)| *decorator));
        GraphNode {
            id: self.runner,
            function: PARALLEL_FUNCTION_ID,
            children: Some(children),
            ..GraphNode::default()
        }
    }

    /// Apply `diff` to the scaffold graph and mark it for re-lowering.
    fn edit(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        self.graph.apply(diff).map_err(|e| BehaviorError {
            message: format!("graph diff: {e}"),
        })?;
        self.dirty = true;
        Ok(())
    }

    /// Prune every finished fragment (their latches would reset on the coming
    /// re-lowering); part of the dirty path.
    fn prune_finished(&mut self) -> Result<(), BehaviorError> {
        if self.finished.is_empty() {
            return Ok(());
        }
        let remove_nodes = self
            .finished
            .drain(..)
            .flat_map(|(decorator, call_node)| [decorator, call_node])
            .collect();
        let diff = GraphDiff {
            remove_nodes,
            add_nodes: vec![self.runner_node()],
            ..GraphDiff::default()
        };
        self.edit(diff)
    }

    /// Process the halts requested since the last tick: write the terminal
    /// status (halted runs end [`Status::Failure`] — the observer that issued
    /// the halt is the one reporting a cancel) and prune the fragments.
    fn process_halts(&mut self, store: &dyn DataStore) -> Result<(), BehaviorError> {
        if self.pending_halts.is_empty() {
            return Ok(());
        }
        let mut remove_nodes = Vec::new();
        for task in std::mem::take(&mut self.pending_halts) {
            // Unknown or already-finished runs are a no-op: halt is idempotent.
            let Some(run) = self.runs.remove(&task) else {
                continue;
            };
            let failure: Value = Status::Failure.into();
            store
                .write(StateChange::set(run.status_key.clone(), failure))
                .map_err(|e| BehaviorError {
                    message: format!("task run {}: writing halt status: {e}", task.0),
                })?;
            remove_nodes.push(run.decorator);
            remove_nodes.push(run.call_node);
        }
        if remove_nodes.is_empty() {
            return Ok(());
        }
        let diff = GraphDiff {
            remove_nodes,
            add_nodes: vec![self.runner_node()],
            ..GraphDiff::default()
        };
        self.edit(diff)
    }

    /// Move every run whose status key reached a terminal state to the
    /// finished list: its fragment is latched inert, pruned at the next edit.
    fn sweep_terminal_runs(&mut self, store: &dyn DataStore) {
        let terminal: Vec<TaskId> = self
            .runs
            .iter()
            .filter(|(_, run)| {
                let value = store.read(std::slice::from_ref(&run.status_key));
                matches!(
                    value.first().and_then(|v| v.clone()).map(Status::try_from),
                    Some(Ok(Status::Success)) | Some(Ok(Status::Failure))
                )
            })
            .map(|(task, _)| *task)
            .collect();
        for task in terminal {
            let run = self.runs.remove(&task).expect("the id came from the map");
            self.finished.push((run.decorator, run.call_node));
        }
    }
}

impl BehaviorInterpreter for BehaviorTreeInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        // Requested halts first: their terminal status is written (this tick
        // owns the store) and their fragments leave the graph.
        self.process_halts(ctx.store)?;

        // The graph changed: replace the lowering. The old registration is
        // undone first; finished fragments leave now (re-lowering would reset
        // their latches and re-fire them). A lowering error is transient — the
        // graph stays, the next tick retries — surfaced like any failed tick.
        if self.dirty {
            self.prune_finished()?;
            if let Some(lowered) = self.lowered.take() {
                lowered.unregister(ctx.call_bridge);
            }
            let tree =
                build_behavior_tree(&self.graph, &direct_resolver(ctx.store)).map_err(|e| {
                    BehaviorError {
                        message: format!("lowering the scaffold: {e:?}"),
                    }
                })?;
            self.lowered = Some(
                lower_behavior_tree(tree, self.function_index.clone(), ctx.call_bridge, false)
                    .map_err(|e| BehaviorError {
                        message: format!("registering the scaffold: {e:?}"),
                    })?,
            );
            self.dirty = false;
        }

        // One runtime tick is one tree tick. The runner's own status is not an
        // interpreter outcome — the scaffold is a standing policy, so the
        // interpreter stays `Running` (it is never dropped).
        if let Some(lowered) = &self.lowered {
            lowered.tick(ctx.call_bridge).map_err(|e| BehaviorError {
                message: format!("behavior tree: {e:?}"),
            })?;
        }

        // Runs that reached a terminal status this tick latch and await
        // pruning; their ids stop resolving (halting them is now a no-op).
        self.sweep_terminal_runs(ctx.store);

        Ok(BehaviorStatus::Running)
    }

    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        // An editor's edit, onto the scaffold. Two adjustments keep the
        // scaffold invariant (the root is always the runner):
        // - `set_root` means "the main behavior's root is now X" — the runner
        //   re-parents it rather than being dethroned;
        // - the runner itself cannot be removed.
        let mut diff = diff;
        diff.remove_nodes.retain(|id| *id != self.runner);
        for id in &diff.remove_nodes {
            self.main.retain(|main| main != id);
            if self.main_root == Some(*id) {
                self.main_root = None;
            }
        }
        for node in &diff.add_nodes {
            if node.id != self.runner && !self.main.contains(&node.id) {
                self.main.push(node.id);
            }
        }
        if let Some(root) = diff.set_root.take() {
            self.main_root = Some(root);
        }
        // A fresh main root (or a removed one) re-parents under the runner.
        diff.add_nodes.push(self.runner_node());
        self.edit(diff)
    }

    fn load(&mut self, graph: Graph) -> Result<(), BehaviorError> {
        // A whole-behavior replacement: the previous main subtree leaves the
        // scaffold, the new behavior grafts under the runner. Live task runs
        // are untouched — they belong to their callers, not to the behavior.
        let mut diff = GraphDiff {
            remove_nodes: std::mem::take(&mut self.main),
            ..GraphDiff::default()
        };
        self.main = graph.nodes.keys().copied().collect();
        self.main_root = graph.root;
        let load = GraphDiff::load(graph);
        diff.add_nodes = load.add_nodes;
        diff.add_links = load.add_links;
        diff.variables = load.variables;
        // `load.set_root` is deliberately not applied: the scaffold root stays
        // the runner; the loaded root becomes the runner's main child.
        diff.add_nodes.push(self.runner_node());
        self.edit(diff)
    }

    fn spawn(&mut self, call: Call, policy: RunPolicy) -> Result<TaskHandle, BehaviorError> {
        // v1 runs every task concurrently — the parallel runner's ordinary
        // semantics (overlapping actuation writes are last-write-wins). Richer
        // `RunPolicy` arbitration lands as visible runner structure later; the
        // policy is accepted and treated as `Concurrent` until then.
        let _ = policy;
        let task = TaskId(Uuid::new_v4());
        let module = call
            .module_id
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".to_string());
        let prefix = format!("arora/tasks/{module}/{}/{}", call.id, task.0);
        let status_key = Key::from(format!("{prefix}/status"));

        // The fragment: a run-status decorator over a run-call leaf. The call
        // is literal link data; the status output is predetermined to the
        // run's status key (the Direct convention binds it to the store).
        let call_value = value_serde::to_value(&call).map_err(|e| BehaviorError {
            message: format!("the spawned call does not convert to a value: {e}"),
        })?;
        let call_node = Uuid::new_v4();
        let decorator = Uuid::new_v4();
        let mut diff = GraphDiff {
            add_nodes: vec![
                GraphNode {
                    id: call_node,
                    function: RUN_CALL_FUNCTION_ID,
                    inputs: vec![Io::new(RUN_CALL_PARAM_ID)],
                    ..GraphNode::default()
                },
                GraphNode {
                    id: decorator,
                    function: RUN_STATUS_FUNCTION_ID,
                    inputs: vec![Io::new(RUN_STATUS_LATCH_PARAM_ID)],
                    outputs: vec![Io {
                        predetermined_key: Some(status_key.path.clone()),
                        ..Io::new(RUN_STATUS_OUT_PARAM_ID)
                    }],
                    children: Some(vec![call_node]),
                },
            ],
            add_links: vec![
                Link::new(
                    Port::new(call_node, RUN_CALL_PARAM_ID),
                    LinkSource::Literal(call_value),
                ),
                Link::new(
                    Port::new(decorator, RUN_STATUS_LATCH_PARAM_ID),
                    LinkSource::Literal(Value::Unit),
                ),
            ],
            ..GraphDiff::default()
        };
        self.runs.insert(
            task,
            Run {
                decorator,
                call_node,
                status_key: status_key.clone(),
            },
        );
        diff.add_nodes.push(self.runner_node());
        self.edit(diff).inspect_err(|_| {
            self.runs.remove(&task);
        })?;

        Ok(TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: status_key,
            feedback: vec![Key::from(format!("{prefix}/feedback"))],
            result: vec![Key::from(format!("{prefix}/result"))],
            update: vec![Key::from(format!("{prefix}/update"))],
        })
    }

    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        // Applied on the next tick, which owns the store: the run's terminal
        // status is written and its fragment pruned. Idempotent — halting an
        // unknown or finished run is a clean no-op.
        self.pending_halts.push(task);
        Ok(())
    }
}
