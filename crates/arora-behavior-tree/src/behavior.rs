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
//! - [`spawn_graph`](BehaviorInterpreter::spawn_graph) grafts a fragment whose
//!   decorator sits over the spawned graph's root, with every node of that
//!   graph;
//! - [`halt`](BehaviorInterpreter::halt)ing prunes the fragment.
//!
//! A run is known by its root. Spawning a graph whose root a live run has
//! grafts nothing: that run takes the graph as its program — the nodes and
//! bindings that differ change, and the run's nodes the graph does not have
//! leave — and the spawn answers the run's [`TaskHandle`]. The run keeps its
//! id, status key and [`RunPolicy`]. A client that lost a run's [`TaskId`]
//! takes the run over by spawning its graph again, and a spawn of the graph a
//! run already runs changes nothing. The graph names only the run's own nodes
//! and new ones: one the main behavior or another run also holds is refused.
//!
//! The runner ticks the main behavior first, then the runs in the order they
//! were spawned.
//!
//! Everything is ordinary graph data, edited through the same
//! [`apply`](BehaviorInterpreter::apply)/[`GraphDiff`] path an editor uses and
//! visible through [`graph`](BehaviorTreeInterpreter::graph) — the runs, their
//! policies, and the loaded behavior are introspectable *the behavior tree's
//! way*, not hidden interpreter state. A run is the subtree under its
//! decorator: an edit reaches its nodes by id like any other node, a node an
//! edit places under the decorator belongs to the run, and a halt prunes the
//! subtree but for the nodes the main behavior or another run also holds. The
//! main behavior is every other node. The runner and the decorators are the
//! scaffold's own: an edit that changes a decorator, or removes a run's root,
//! is refused — halting is how a run leaves. Concurrency is the parallel
//! runner's ordinary semantics; richer [`RunPolicy`] arbitration lands as
//! runner-node kinds and decorators, still as visible structure.
//!
//! # Graph type
//!
//! The interpreter reads the `behavior-tree` language ([`GRAPH_TYPE_NAME`]) at
//! version [`GRAPH_TYPE_VERSION`]: [`spawn_graph`](BehaviorInterpreter::spawn_graph)
//! runs a graph written for that version or an older minor of its major, and
//! refuses any other.
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
//! outcome, and the fragment sits inert until the next tick or the next call
//! that changes the graph prunes it. A halted run's fragment leaves the graph
//! at once, and its `Failure` status is written on the next tick. A run whose
//! program errors ends `Failure` too: the error is the tick's, once, and the
//! other runs tick on.
//!
//! The lowered tree persists across ticks and is rebuilt only when the graph
//! changed (an edit, a load, a spawn, a halt, a finished run pruned),
//! unregistering the previous lowering's engine callables. The rebuilt tree
//! keeps the state of every node the change left in place — a `seq_star`'s
//! index, a run's latch, a tree-local variable such a node reads — so changing
//! one run leaves the main behavior and every other run where they were. A
//! node the change sets, or whose bindings it changes (a binding reading a
//! node it removes goes with that node), starts afresh, and so
//! does a tree-local variable only such nodes read. A load thus restarts the
//! main behavior's nodes, but a tree-local variable a run also reads keeps its
//! value, as the run left in place sees it; an edit restarts the nodes it
//! touches. A graph that does not lower leaves the previous lowering in place,
//! unticked, and the next lowering takes over its state.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use arora_behavior::graph::{source_port, Graph, GraphDiff, GraphType, Node as GraphNode, Source};
use arora_behavior::{
    interpreter_module, BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus,
    RunPolicy, TaskHandle, TaskId, TraceOptions,
};
use arora_types::call::Call;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use arora_types::value_serde;
use uuid::Uuid;

use crate::detached::is_detachable;
use crate::graph::build_behavior_tree;
use crate::is_interpreter_function_id;
use crate::nodes::{
    PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID, RUN_CALL_PARAM_ID, RUN_STATUS_FUNCTION_ID,
    RUN_STATUS_LATCH_PARAM_ID, RUN_STATUS_OUT_PARAM_ID,
};
use crate::schema::_RET_PARAM_ID;
use crate::trace::Traces;
use crate::variable::VariableResolver;
use crate::{lower_behavior_tree, schema_groot, FunctionIndex, LoweredTree};
use arora_behavior::Status;

/// The name of the language this interpreter reads: the [`GraphType::name`] a
/// graph spawned on it states.
pub const GRAPH_TYPE_NAME: &str = "behavior-tree";

/// The version of the `behavior-tree` format this interpreter reads. A graph
/// written for it, or for an older minor version of its major, runs.
///
/// `2.0` is `arora-behavior` 10's graph model, a node holding its own
/// bindings and a binding naming its key: the control nodes, the status
/// leaves, the data nodes `Equal`, `Unpack` and `Scatter`
/// ([`EQUAL_FUNCTION_ID`](crate::nodes::EQUAL_FUNCTION_ID),
/// [`UNPACK_FUNCTION_ID`](crate::nodes::UNPACK_FUNCTION_ID),
/// [`SCATTER_FUNCTION_ID`](crate::nodes::SCATTER_FUNCTION_ID)), the task-run
/// nodes, module function leaves, and the calls a graph lists and no node
/// holds, run as [detached nodes](crate::detached).
pub const GRAPH_TYPE_VERSION: semver::Version = semver::Version::new(2, 0, 0);

/// The [`GraphType`] this interpreter reads: [`GRAPH_TYPE_NAME`] at
/// [`GRAPH_TYPE_VERSION`]. A graph states the oldest version that has every
/// node it uses, so that an interpreter reading an older minor still runs it.
pub fn graph_type() -> GraphType {
    GraphType {
        name: GRAPH_TYPE_NAME.to_string(),
        version: GRAPH_TYPE_VERSION,
    }
}

/// One task run's place in the scaffold: its decorator, and the handle its
/// spawn answered, whose status key the decorator publishes to. The run's
/// nodes are the decorator's subtree in the graph; this is the bookkeeping
/// that maps a [`TaskId`] onto it.
struct Run {
    handle: TaskHandle,
    decorator: Uuid,
    /// The nodes of the run's graph that no node holds: detached, they leave
    /// with the run.
    detached: Vec<Uuid>,
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
    /// The loaded main behavior's root (the runner's first child).
    main_root: Option<Uuid>,
    /// Live task runs, in spawn order: the order the runner ticks them in.
    runs: Vec<Run>,
    /// Finished runs awaiting pruning: latched inert, removed at the next
    /// change of the graph.
    finished: Vec<Run>,
    /// The status keys of the runs halted since the last tick: the next tick
    /// (which owns the store) writes their terminal status.
    halted: Vec<Key>,
    /// The persistent lowering; rebuilt when [`dirty`](Self::dirty).
    lowered: Option<LoweredTree>,
    /// The graph changed since the last lowering.
    dirty: bool,
    /// The nodes the graph's changes since the last lowering set, or whose
    /// bindings they changed: they start afresh at the next lowering, and
    /// every other node keeps its state.
    fresh: HashSet<Uuid>,
    /// The device's function index as of the last lowering: a run's detached
    /// nodes are lowered against it before the run is accepted.
    functions: FunctionIndex,
    /// The running traces.
    traces: Traces,
    /// The ticks so far: the tick a trace's report names.
    ticks: u64,
}

/// Bind a `{var}` name to the store slot under that name — the Direct
/// convention (variable name == store key). The lowering never asks it for a
/// tree-local name ([`is_tree_local`](crate::variable::is_tree_local)). It
/// holds its own handle on the store, so the lowered tree keeps it to resolve
/// the keys its `Scatter` nodes are handed.
fn direct_resolver(store: &dyn DataStore) -> Rc<VariableResolver<'static>> {
    let store = store.clone_box();
    Rc::new(move |name: &str| Some(store.slot(&Key::from(name))))
}

/// The nodes of `graph`, by id.
fn by_id(graph: &Graph) -> Index<'_> {
    graph.nodes.iter().map(|node| (node.id, node)).collect()
}

/// A graph's nodes by id, built once for each operation that walks them.
type Index<'a> = HashMap<Uuid, &'a GraphNode>;

/// `from` and the nodes under it, through the children relation of `nodes`.
fn subtree(nodes: &Index, from: Uuid) -> HashSet<Uuid> {
    reach(nodes, [from])
}

/// Check that `graph` lists each node once, and is one tree rooted at `root`,
/// beside which it may list detached nodes: every child is one of its nodes,
/// no node has two parents or is the root's parent, and every node is reached
/// from the root, but for a node no node holds that is a call to a module
/// function with no children, which runs [detached](crate::detached). Returns
/// the detached nodes.
fn check_tree(graph: &Graph, root: Uuid) -> Result<Vec<Uuid>, BehaviorError> {
    let nodes = by_id(graph);
    if nodes.len() != graph.nodes.len() {
        return Err(listed_twice(graph));
    }
    let mut parent: HashMap<Uuid, Uuid> = HashMap::new();
    for node in &graph.nodes {
        for child in node.children.iter().flatten() {
            if !nodes.contains_key(child) {
                return Err(behavior_error(format!(
                    "node {} has child {child}, which is not one of the graph's nodes",
                    node.id
                )));
            }
            if *child == root {
                return Err(behavior_error(format!(
                    "node {} has the graph's root {root} as a child",
                    node.id
                )));
            }
            if let Some(other) = parent.insert(*child, node.id) {
                return Err(behavior_error(format!(
                    "node {child} is a child of both {other} and {}",
                    node.id
                )));
            }
        }
    }
    let mut detached = Vec::new();
    for node in &graph.nodes {
        if node.id == root || parent.contains_key(&node.id) {
            continue;
        }
        if !is_detachable(node.function, node.children.as_deref()) {
            return Err(behavior_error(format!(
                "node {} is not under the graph's root {root}: a node outside the tree runs \
                 detached only when it calls a module function and has no children",
                node.id
            )));
        }
        detached.push(node.id);
    }
    // One parent each and a parentless root: a cycle would leave its nodes
    // unreachable from the root.
    if subtree(&nodes, root).len() + detached.len() != graph.nodes.len() {
        return Err(behavior_error("the graph's children form a cycle"));
    }
    detached.sort();
    Ok(detached)
}

/// The refusal of a graph that lists a node twice, naming the first such.
fn listed_twice(graph: &Graph) -> BehaviorError {
    let mut seen = HashSet::new();
    let twice = graph
        .nodes
        .iter()
        .find(|node| !seen.insert(node.id))
        .map_or(Uuid::nil(), |node| node.id);
    behavior_error(format!("node {twice} is listed twice"))
}

/// Check that the nodes under `root` name only nodes as children and hold no
/// cycle. A node may sit under several parents.
fn check_acyclic(graph: &Graph, root: Uuid) -> Result<(), BehaviorError> {
    let nodes = by_id(graph);
    // Depth-first, each node once: `open` holds the path being walked, so a
    // child on it closes a cycle.
    let mut done = HashSet::new();
    let mut open = HashSet::from([root]);
    let mut stack = vec![(root, 0usize)];
    while let Some((id, next)) = stack.last_mut() {
        let children = nodes[id].children.as_deref().unwrap_or_default();
        let Some(child) = children.get(*next).copied() else {
            open.remove(id);
            done.insert(*id);
            stack.pop();
            continue;
        };
        *next += 1;
        if !nodes.contains_key(&child) {
            return Err(behavior_error(format!(
                "node {id} has child {child}, which is not a node"
            )));
        }
        if open.contains(&child) {
            return Err(behavior_error(format!(
                "node {child} is its own descendant"
            )));
        }
        if !done.contains(&child) {
            open.insert(child);
            stack.push((child, 0));
        }
    }
    Ok(())
}

/// Check that no selection is written: a `Select` binding reads part of its
/// source, so it cannot bind a node's return (`_ret`) or a parameter its
/// function declares mutable, whose writes would land on the whole source.
fn check_selections(graph: &Graph, index: &FunctionIndex) -> Result<(), BehaviorError> {
    for node in &graph.nodes {
        for (port, source) in &node.bind {
            if !matches!(source, Source::Select { .. }) {
                continue;
            }
            let written = *port == _RET_PARAM_ID
                || index
                    .get(&node.function)
                    .and_then(|function| function.function.parameters.get(port))
                    .is_some_and(|parameter| parameter.mutable);
            if written {
                return Err(behavior_error(format!(
                    "the selection bound to {}.{port} would be written: a selection is \
                     read-only, so bind a key there",
                    node.id
                )));
            }
        }
    }
    Ok(())
}

/// The nodes `starts` reach through the children relation of `nodes`,
/// `starts` included.
fn reach(nodes: &Index, starts: impl IntoIterator<Item = Uuid>) -> HashSet<Uuid> {
    let mut found = HashSet::new();
    let mut pending: Vec<Uuid> = starts.into_iter().collect();
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        let children = nodes.get(&id).and_then(|node| node.children.as_ref());
        pending.extend(children.into_iter().flatten());
    }
    found
}

/// The nodes `diff` changes but does not remove: those it sets, and those
/// whose bindings it changes. Every field of a diff is named, so a new one
/// must be placed here.
fn changed_nodes(diff: &GraphDiff) -> impl Iterator<Item = Uuid> + '_ {
    let GraphDiff {
        remove: _,
        set,
        unbind,
        bind,
        root: _,
    } = diff;
    set.iter()
        .map(|node| node.id)
        .chain(unbind.iter().map(|port| port.node))
        .chain(bind.iter().map(|binding| binding.node))
}

/// The nodes of `graph` that removing `remove` leaves but unbinds: those
/// with a binding reading a removed node, which goes with it as an unbind
/// would take it.
fn unbound_by_removal<'a>(graph: &'a Graph, remove: &'a [Uuid]) -> impl Iterator<Item = Uuid> + 'a {
    graph
        .nodes
        .iter()
        .filter(|node| !remove.contains(&node.id))
        .filter(|node| {
            node.bind
                .values()
                .any(|source| source_port(source).is_some_and(|port| remove.contains(&port.node)))
        })
        .map(|node| node.id)
}

fn behavior_error(message: impl Into<String>) -> BehaviorError {
    BehaviorError {
        message: message.into(),
    }
}

impl Default for BehaviorTreeInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl BehaviorTreeInterpreter {
    /// Construct the interpreter. It hosts the runner scaffold immediately;
    /// with nothing loaded the runner ticks no children and the interpreter
    /// idles. The module functions its call nodes reach come from the device's
    /// index, which each tick hands over ([`BehaviorContext::functions`]).
    pub fn new() -> Self {
        let runner = Uuid::new_v4();
        let mut graph = Graph::empty();
        graph.nodes.push(GraphNode {
            id: runner,
            function: PARALLEL_FUNCTION_ID,
            children: Some(Vec::new()),
            ..GraphNode::default()
        });
        graph.root = Some(runner);
        Self {
            graph,
            runner,
            main_root: None,
            runs: Vec::new(),
            finished: Vec::new(),
            halted: Vec::new(),
            lowered: None,
            dirty: true,
            fresh: HashSet::new(),
            functions: FunctionIndex::new(),
            traces: Traces::default(),
            ticks: 0,
        }
    }

    /// Load a behavior tree from Groot XML, replacing the main behavior. The
    /// XML lowers onto the shared [`Graph`] (names → arora ids, resolved
    /// against `functions`; `{var}` → the key `var`) and grafts through
    /// [`load`](BehaviorInterpreter::load), so a Groot-loaded behavior is
    /// editable and introspectable like any other.
    pub fn load_groot(
        &mut self,
        xml: &str,
        functions: &FunctionIndex,
    ) -> Result<(), crate::error::BehaviorTreeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)?;
        let graph = groot.into_graph(functions)?;
        self.load(graph)
            .map_err(|e| crate::error::BehaviorTreeError::InconsistentTreeError {
                message: e.to_string(),
            })
    }

    /// Whether the lowered tree records what its ticks do.
    #[cfg(test)]
    pub(crate) fn records(&self) -> bool {
        self.lowered
            .as_ref()
            .is_some_and(|lowered| lowered.tree().record.is_on())
    }

    /// The scaffold graph: the runner, the loaded behavior, and every task-run
    /// fragment — the interpreter's whole live structure, as data.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The runner node with its children recomputed from the current state:
    /// the main behavior's root first, then every live run's decorator in
    /// spawn order, then the finished ones'. Re-adding it (same id) in a diff
    /// replaces it.
    fn runner_node(&self) -> GraphNode {
        let mut children = Vec::with_capacity(1 + self.runs.len() + self.finished.len());
        children.extend(self.main_root);
        children.extend(self.runs.iter().map(|run| run.decorator));
        children.extend(self.finished.iter().map(|run| run.decorator));
        GraphNode {
            id: self.runner,
            function: PARALLEL_FUNCTION_ID,
            children: Some(children),
            ..GraphNode::default()
        }
    }

    /// The root of `run`'s program: its decorator's one child, in `nodes`,
    /// the graph's index.
    fn root_of(nodes: &Index, run: &Run) -> Option<Uuid> {
        nodes
            .get(&run.decorator)
            .and_then(|decorator| decorator.children.as_deref())
            .and_then(|children| children.first().copied())
    }

    /// Every run, live then finished.
    fn all_runs(&self) -> impl Iterator<Item = &Run> {
        self.runs.iter().chain(&self.finished)
    }

    /// The run whose decorator is `id`, if any.
    fn decorated_by(&self, id: &Uuid) -> Option<&Run> {
        self.all_runs().find(|run| run.decorator == *id)
    }

    /// `run`'s nodes: its decorator, its subtree in `nodes`, the graph's
    /// index, and its detached nodes.
    fn nodes_of(nodes: &Index, run: &Run) -> HashSet<Uuid> {
        let mut held = subtree(nodes, run.decorator);
        held.extend(&run.detached);
        held
    }

    /// Every run's nodes.
    fn run_nodes(&self) -> HashSet<Uuid> {
        let nodes = by_id(&self.graph);
        self.all_runs()
            .flat_map(|run| Self::nodes_of(&nodes, run))
            .collect()
    }

    /// Apply `diff` to the scaffold graph and mark it for re-lowering. The
    /// diff applies whole or not at all: it is refused when it fails, when the
    /// tree under the runner would name a child that is not a node or hold a
    /// cycle, when a selection would bind a return, or when the graph would
    /// not lower. A selection bound to a parameter the function declares
    /// mutable fails the next tick, which holds the device's index.
    fn edit(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        self.edit_lowering(diff, false)
    }

    /// [`edit`](Self::edit), and when `detached`, lower the edited graph's
    /// detached nodes too, against the function index of the last lowering:
    /// a run whose detached nodes cannot lower — reading one another in a
    /// cycle, writing one cell twice — is refused here, rather than stalling
    /// every tick of the device until it is halted. A function that index
    /// does not have fails its node when it runs, as at any lowering.
    fn edit_lowering(&mut self, diff: GraphDiff, detached: bool) -> Result<(), BehaviorError> {
        let mut changed: Vec<Uuid> = changed_nodes(&diff).collect();
        changed.extend(unbound_by_removal(&self.graph, &diff.remove));
        let mut graph = self.graph.clone();
        graph
            .apply(diff)
            .map_err(|e| behavior_error(format!("graph diff: {e}")))?;
        check_acyclic(&graph, self.runner)?;
        // Whether a parameter is mutable is the device's index's to say, which
        // the tick hands over: here only the return binding is checked, and
        // the lowering checks the rest.
        check_selections(&graph, &FunctionIndex::new())?;
        let tree = build_behavior_tree(&graph, &|_| None)
            .map_err(|e| behavior_error(format!("the edited graph does not lower: {e:?}")))?;
        if detached {
            tree.lower_detached(&self.functions).map_err(|e| {
                behavior_error(format!("the graph's detached nodes do not lower: {e:?}"))
            })?;
        }
        self.graph = graph;
        self.dirty = true;
        self.fresh.extend(changed);
        Ok(())
    }

    /// Graft a run under the runner: a run-status decorator over `root`, with
    /// `fragment` — the run's nodes — added beside it. The run's keys live
    /// under `arora/tasks/<module>/<function>/<run>/`.
    fn graft(
        &mut self,
        module: &str,
        function: Uuid,
        root: Uuid,
        detached: Vec<Uuid>,
        mut fragment: Vec<GraphNode>,
    ) -> Result<TaskHandle, BehaviorError> {
        let task = TaskId(Uuid::new_v4());
        let prefix = format!("arora/tasks/{module}/{function}/{}", task.0);
        let status_key = Key::from(format!("{prefix}/status"));

        // The decorator writes its status to the run's status key; its latch
        // starts empty.
        let decorator = Uuid::new_v4();
        fragment.push(GraphNode {
            id: decorator,
            function: RUN_STATUS_FUNCTION_ID,
            bind: [
                (RUN_STATUS_LATCH_PARAM_ID, Source::Value(Value::Unit)),
                (
                    RUN_STATUS_OUT_PARAM_ID,
                    Source::Key(status_key.path.clone()),
                ),
            ]
            .into(),
            children: Some(vec![root]),
        });
        let handle = TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: status_key,
            feedback: vec![Key::from(format!("{prefix}/feedback"))],
            result: vec![Key::from(format!("{prefix}/result"))],
            update: vec![Key::from(format!("{prefix}/update"))],
        };
        self.runs.push(Run {
            handle: handle.clone(),
            decorator,
            detached,
        });
        fragment.push(self.runner_node());
        let diff = GraphDiff {
            set: fragment,
            ..GraphDiff::default()
        };
        self.edit_lowering(diff, true).inspect_err(|_| {
            self.runs.pop();
        })?;
        Ok(handle)
    }

    /// The nodes something besides `leaving` holds, with what they reach: the
    /// main behavior, from its root even when `leaving` holds that too, and
    /// every node `leaving` does not have.
    fn held_outside(&self, leaving: &HashSet<Uuid>) -> HashSet<Uuid> {
        reach(
            &by_id(&self.graph),
            self.graph
                .nodes
                .iter()
                .map(|node| node.id)
                .filter(|id| *id != self.runner && !leaving.contains(id))
                .chain(self.main_root),
        )
    }

    /// Bring the live run `self.runs[at]`, whose root is `graph`'s, to run
    /// `graph`, and answer the run's handle. Only what differs changes — the
    /// diff from the run's own nodes to the graph: the graph's nodes the run
    /// lacks or holds otherwise, the bindings that differ, and the run's nodes
    /// the graph does not have. A graph the run already runs changes nothing.
    /// The graph may name only the run's own nodes and new ones: a node the
    /// main behavior or another run also holds is theirs as much as the
    /// run's, and stays as they hold it.
    fn respawn(
        &mut self,
        at: usize,
        graph: Graph,
        detached: Vec<Uuid>,
    ) -> Result<TaskHandle, BehaviorError> {
        let decorator = self.runs[at].decorator;
        let device = by_id(&self.graph);
        let mut held = Self::nodes_of(&device, &self.runs[at]);
        held.remove(&decorator);
        let mut leaving = held.clone();
        leaving.insert(decorator);
        let staying = self.held_outside(&leaving);
        if let Some(node) = graph
            .nodes
            .iter()
            .find(|node| device.contains_key(&node.id) && !held.contains(&node.id))
        {
            return Err(behavior_error(format!(
                "node {} is already part of the device's behavior",
                node.id
            )));
        }
        if let Some(node) = graph.nodes.iter().find(|node| staying.contains(&node.id)) {
            return Err(behavior_error(format!(
                "node {} is held by the main behavior or another run too: a graph spawned \
                 again with its run's root names only the run's own nodes and new ones",
                node.id
            )));
        }

        // The run's own nodes, as a graph rooted where the spawned one is.
        let own = Graph {
            root: graph.root,
            nodes: self
                .graph
                .nodes
                .iter()
                .filter(|node| held.contains(&node.id) && !staying.contains(&node.id))
                .cloned()
                .collect(),
        };
        let diff = own.diff(&graph);
        if diff != GraphDiff::default() {
            self.edit_lowering(diff, true)?;
        }
        let run = &mut self.runs[at];
        run.detached = detached;
        Ok(run.handle.clone())
    }

    /// Prune `runs`, already taken out of the live and finished lists: each
    /// decorator and its subtree leave the graph, but for the nodes another
    /// node — of the main behavior or of a remaining run — also holds.
    fn prune(&mut self, runs: Vec<Run>) -> Result<(), BehaviorError> {
        if runs.is_empty() {
            return Ok(());
        }
        let mut leaving = reach(&by_id(&self.graph), runs.iter().map(|run| run.decorator));
        leaving.extend(runs.iter().flat_map(|run| &run.detached));
        let staying = self.held_outside(&leaving);
        let diff = GraphDiff {
            remove: leaving.difference(&staying).copied().collect(),
            set: vec![self.runner_node()],
            ..GraphDiff::default()
        };
        self.edit(diff)
    }

    /// Write the terminal status of the runs halted since the last tick:
    /// halted runs end [`Status::Failure`] — the observer that issued the halt
    /// is the one reporting a cancel.
    fn write_halts(&mut self, store: &dyn DataStore) -> Result<(), BehaviorError> {
        for key in std::mem::take(&mut self.halted) {
            let failure: Value = Status::Failure.into();
            store
                .write(StateChange::set(key.clone(), failure))
                .map_err(|e| behavior_error(format!("{key}: writing halt status: {e}")))?;
        }
        Ok(())
    }

    /// Prune the runs that finished since the last change of the graph, which
    /// is changing anyway: until then their latches hold them inert.
    fn prune_finished(&mut self) -> Result<(), BehaviorError> {
        let finished = std::mem::take(&mut self.finished);
        self.prune(finished)
    }

    /// Move every run whose status key reached a terminal state to the
    /// finished list: its fragment is latched inert, pruned at the next edit.
    fn sweep_terminal_runs(&mut self, store: &dyn DataStore) {
        let terminal = |run: &Run| {
            let value = store.read(std::slice::from_ref(&run.handle.status));
            matches!(
                value.first().and_then(|v| v.clone()).map(Status::try_from),
                Some(Ok(Status::Success)) | Some(Ok(Status::Failure))
            )
        };
        let (finished, live) = std::mem::take(&mut self.runs)
            .into_iter()
            .partition::<Vec<_>, _>(terminal);
        self.runs = live;
        // The next tick prunes them.
        self.dirty |= !finished.is_empty();
        self.finished.extend(finished);
    }
}

impl BehaviorInterpreter for BehaviorTreeInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        self.ticks += 1;
        // The halted runs' terminal status first: this tick owns the store.
        self.write_halts(ctx.store)?;

        // The graph changed: replace the lowering. Finished fragments leave
        // first. The new tree is lowered beside the old one; once it is, it
        // takes over the old one's state but for the fresh nodes, then the old
        // registration is undone. A lowering error is transient — the graph
        // stays, the next tick retries — surfaced like any failed tick, and
        // leaves the old lowering, its state and the fresh nodes as they were.
        if self.dirty {
            self.prune_finished()?;
            check_selections(&self.graph, ctx.functions)?;
            let resolver = direct_resolver(ctx.store);
            let tree = build_behavior_tree(&self.graph, &*resolver)
                .map_err(|e| behavior_error(format!("lowering the scaffold: {e:?}")))?
                .with_key_resolver(resolver);
            let lowered = lower_behavior_tree(tree, ctx.functions, ctx.call_bridge, false)
                .map_err(|e| behavior_error(format!("registering the scaffold: {e:?}")))?;
            if let Some(previous) = self.lowered.take() {
                lowered.tree().keep_state(previous.tree(), &self.fresh);
                previous.unregister(ctx.call_bridge);
                // The nodes the change restarted, that were and still are in
                // the tree: the traces state them anew. The runner is set
                // again by every change, to list its children; it holds no
                // state.
                if !self.traces.is_empty() {
                    let mut restarted: Vec<Uuid> = self
                        .fresh
                        .iter()
                        .filter(|id| {
                            **id != self.runner
                                && previous.tree().node_index.contains_key(id)
                                && lowered.tree().node_index.contains_key(id)
                        })
                        .copied()
                        .collect();
                    restarted.sort_unstable();
                    self.traces.restart(&restarted);
                }
            }
            self.traces.arm(&lowered.tree().record);
            self.fresh.clear();
            self.lowered = Some(lowered);
            self.dirty = false;
            self.functions.clone_from(ctx.functions);
        }

        // One runtime tick is one tree tick. The runner's own status is not an
        // interpreter outcome — the scaffold is a standing policy, so the
        // interpreter stays `Running` (it is never dropped).
        let ticked = match &self.lowered {
            Some(lowered) => lowered
                .tick(ctx.call_bridge)
                .map(|_| ())
                .map_err(|e| behavior_error(format!("behavior tree: {e:?}"))),
            None => Ok(()),
        };

        // What the tick did, to the traces: a failed tick's too.
        if !self.traces.is_empty() {
            if let Some(lowered) = &self.lowered {
                self.traces
                    .report(
                        self.ticks,
                        &lowered.tree().record,
                        Some(lowered.tree()),
                        &self.functions,
                        ctx.store,
                    )
                    .map_err(behavior_error)?;
            }
        }
        ticked?;

        // Runs that reached a terminal status this tick latch and await
        // pruning; their ids stop resolving (halting them is now a no-op).
        self.sweep_terminal_runs(ctx.store);

        Ok(BehaviorStatus::Running)
    }

    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        // An editor's edit, onto the scaffold. It keeps the scaffold's
        // invariants — the root is always the runner, and every run hangs from
        // its decorator:
        // - `root` means "the main behavior's root is now X" — the runner
        //   re-parents it rather than being dethroned; removing the main
        //   behavior's root unroots it, as removing a graph's root does;
        // - the runner cannot be removed;
        // - an edit that changes a decorator, or removes a run's root, is
        //   refused: halting is how a run leaves.
        self.prune_finished()?;
        let mut diff = diff;
        let scaffold = |id: &Uuid| *id == self.runner || self.decorated_by(id).is_some();
        if let Some(id) = diff
            .root
            .iter()
            .chain(
                diff.set
                    .iter()
                    .flat_map(|node| node.children.iter().flatten()),
            )
            .find(|id| scaffold(id))
        {
            return Err(behavior_error(format!(
                "node {id} is the runner or a task run's decorator: an edit cannot place it"
            )));
        }
        let touched = changed_nodes(&diff).chain(diff.remove.iter().copied());
        for id in touched {
            if let Some(run) = self.decorated_by(&id) {
                return Err(behavior_error(format!(
                    "node {id} is the decorator of task run {}: an edit cannot change it",
                    run.handle.id.0
                )));
            }
        }
        let nodes = by_id(&self.graph);
        for run in self.all_runs() {
            if let Some(root) = Self::root_of(&nodes, run).filter(|root| {
                diff.remove.contains(root) && !diff.set.iter().any(|node| node.id == *root)
            }) {
                return Err(behavior_error(format!(
                    "node {root} is the root of task run {}: halt the run to remove it",
                    run.handle.id.0
                )));
            }
        }

        diff.remove.retain(|id| *id != self.runner);
        // A fresh main root (or a removed one) re-parents under the runner;
        // a refused edit leaves the main root as it was.
        let previous = self.main_root;
        if self
            .main_root
            .is_some_and(|root| diff.remove.contains(&root))
        {
            self.main_root = None;
        }
        if let Some(root) = diff.root.take() {
            self.main_root = Some(root);
        }
        diff.set.push(self.runner_node());
        self.edit(diff).inspect_err(|_| self.main_root = previous)
    }

    fn load(&mut self, graph: Graph) -> Result<(), BehaviorError> {
        // A whole-behavior replacement: the previous main behavior — every node
        // that is not the scaffold's or a run's — leaves the scaffold, the new
        // behavior grafts under the runner. Live task runs are untouched — they
        // belong to their callers, not to the behavior — so the new behavior
        // may not reuse their nodes.
        self.prune_finished()?;
        if by_id(&graph).len() != graph.nodes.len() {
            return Err(listed_twice(&graph));
        }
        let run_nodes = self.run_nodes();
        if let Some(node) = graph
            .nodes
            .iter()
            .find(|node| node.id == self.runner || run_nodes.contains(&node.id))
        {
            return Err(behavior_error(format!(
                "node {} belongs to the runner or a task run: a loaded behavior cannot \
                 replace it",
                node.id
            )));
        }
        let remove = self
            .graph
            .nodes
            .iter()
            .map(|node| node.id)
            .filter(|id| *id != self.runner && !run_nodes.contains(id))
            .collect();
        // The loaded root becomes the runner's main child: the scaffold root
        // stays the runner. A refused load leaves the main root as it was.
        let previous = std::mem::replace(&mut self.main_root, graph.root);
        let mut set = graph.nodes;
        set.push(self.runner_node());
        self.edit(GraphDiff {
            remove,
            set,
            ..GraphDiff::default()
        })
        .inspect_err(|_| self.main_root = previous)
    }

    fn spawn(&mut self, call: Call, policy: RunPolicy) -> Result<TaskHandle, BehaviorError> {
        // v1 runs every task concurrently — the parallel runner's ordinary
        // semantics (overlapping actuation writes are last-write-wins). Richer
        // `RunPolicy` arbitration lands as visible runner structure later; the
        // policy is accepted and treated as `Concurrent` until then.
        let _ = policy;
        let module = call
            .module_id
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".to_string());
        let function = call.id;

        // The run is one run-call leaf; the call is the value its port is
        // bound to.
        let call_value = value_serde::to_value(&call).map_err(|e| {
            behavior_error(format!("the spawned call does not convert to a value: {e}"))
        })?;
        self.prune_finished()?;
        let call_node = Uuid::new_v4();
        let fragment = vec![GraphNode {
            id: call_node,
            function: RUN_CALL_FUNCTION_ID,
            bind: [(RUN_CALL_PARAM_ID, Source::Value(call_value))].into(),
            children: None,
        }];
        self.graft(&module, function, call_node, Vec::new(), fragment)
    }

    fn spawn_graph(
        &mut self,
        graph_type: &GraphType,
        graph: Graph,
        policy: RunPolicy,
    ) -> Result<TaskHandle, BehaviorError> {
        // Concurrent, like every run (see `spawn`).
        let _ = policy;
        let reads = self::graph_type();
        if !reads.reads(graph_type) {
            return Err(behavior_error(format!(
                "the behavior-tree interpreter reads {reads}, not {graph_type}"
            )));
        }
        let root = graph
            .root
            .ok_or_else(|| behavior_error("a spawned graph needs a root"))?;
        if graph.node(&root).is_none() {
            return Err(behavior_error(format!(
                "the spawned graph's root {root} is not one of its nodes"
            )));
        }
        // The run is its own region of the scaffold: one tree, and the
        // detached nodes beside it, of nodes the device does not have (or its
        // own, for a live run's root), of functions the interpreter runs,
        // whose bindings read only among them and lower. The detached nodes
        // leave with the run.
        let detached = check_tree(&graph, root)?;
        if let Some(node) = graph
            .nodes
            .iter()
            .find(|node| is_interpreter_function_id(node.function))
        {
            return Err(behavior_error(format!(
                "node {} calls the interpreter module's function {}, which a tree cannot call",
                node.id, node.function
            )));
        }
        let ids: HashSet<Uuid> = graph.nodes.iter().map(|node| node.id).collect();
        for node in &graph.nodes {
            if let Some((port, _)) = node.bind.iter().find(|(_, source)| {
                source_port(source).is_some_and(|read| !ids.contains(&read.node))
            }) {
                return Err(behavior_error(format!(
                    "the spawned graph's binding of {}.{port} reads outside its nodes",
                    node.id
                )));
            }
        }
        build_behavior_tree(&graph, &|_| None)
            .map_err(|e| behavior_error(format!("the spawned graph does not lower: {e:?}")))?;
        self.prune_finished()?;
        // A run is known by its root: the live run whose root this is runs
        // the graph from now on.
        let nodes = by_id(&self.graph);
        if let Some(at) = self
            .runs
            .iter()
            .position(|run| Self::root_of(&nodes, run) == Some(root))
        {
            return self.respawn(at, graph, detached);
        }
        let device = nodes;
        if let Some(node) = graph
            .nodes
            .iter()
            .find(|node| device.contains_key(&node.id))
        {
            return Err(behavior_error(format!(
                "node {} is already part of the device's behavior",
                node.id
            )));
        }
        self.graft(
            &interpreter_module::ID.to_string(),
            interpreter_module::SPAWN_GRAPH,
            root,
            detached,
            graph.nodes,
        )
    }

    fn trace(&mut self, options: TraceOptions) -> Result<TaskHandle, BehaviorError> {
        // A trace is a run of the interpreter module's trace function: its
        // keys are laid out as a spawned run's.
        let task = TaskId(Uuid::new_v4());
        let prefix = format!(
            "arora/tasks/{}/{}/{}",
            interpreter_module::ID,
            interpreter_module::TRACE,
            task.0
        );
        let handle = TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: Key::from(format!("{prefix}/status")),
            feedback: vec![Key::from(format!("{prefix}/feedback"))],
            result: vec![Key::from(format!("{prefix}/result"))],
            update: vec![Key::from(format!("{prefix}/update"))],
        };
        self.traces.start(handle.clone(), options);
        if let Some(lowered) = &self.lowered {
            self.traces.arm(&lowered.tree().record);
        }
        Ok(handle)
    }

    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        // A trace ends at once; its terminal status is written on the next
        // tick, as a halted run's is.
        if let Some(status) = self.traces.halt(task) {
            if let Some(lowered) = &self.lowered {
                self.traces.arm(&lowered.tree().record);
            }
            self.halted.push(status);
            return Ok(());
        }
        // The fragment leaves the graph now, so a spawn reusing its node ids
        // can follow at once; the terminal status is written on the next
        // tick, which owns the store. Idempotent — halting an unknown or
        // finished run is a clean no-op.
        let Some(at) = self.runs.iter().position(|run| run.handle.id == task) else {
            return Ok(());
        };
        let run = self.runs.remove(at);
        let status_key = run.handle.status.clone();
        self.prune(vec![run])?;
        self.halted.push(status_key);
        Ok(())
    }
}
