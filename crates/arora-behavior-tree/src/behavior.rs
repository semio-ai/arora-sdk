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
//!   decorator sits over the spawned graph's root, with every node and link of
//!   that graph;
//! - [`halt`](BehaviorInterpreter::halt)ing prunes the fragment.
//!
//! A run is known by its root. Spawning a graph whose root a live run has
//! grafts nothing: that run takes the graph as its program — the nodes and
//! links that differ change, and the run's nodes the graph does not have
//! leave — and the spawn answers the run's [`TaskHandle`]. A client that lost
//! a run's [`TaskId`] takes the run over by spawning its graph again, and a
//! spawn of the graph a run already runs changes nothing.
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
//! node the change adds, replaces, relinks or re-keys starts afresh, and so
//! does a tree-local variable only such nodes read. A load thus restarts the
//! main behavior's nodes, but a tree-local variable a run also reads keeps its
//! value, as the run left in place sees it; an edit restarts the nodes it
//! touches. A graph that does not lower leaves the previous lowering in place,
//! unticked, and the next lowering takes over its state.

use std::collections::{HashMap, HashSet};

use arora_behavior::graph::{
    source_port, Graph, GraphDiff, GraphType, Io, Link, LinkSource, Node as GraphNode, Port,
};
use arora_behavior::{
    interpreter_module, BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus,
    RunPolicy, TaskHandle, TaskId,
};
use arora_types::call::Call;
use arora_types::data::{DataStore, Key, Slot, StateChange};
use arora_types::value::Value;
use arora_types::value_serde;
use uuid::Uuid;

use crate::graph::build_behavior_tree;
use crate::is_interpreter_function_id;
use crate::nodes::{
    PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID, RUN_CALL_PARAM_ID, RUN_STATUS_FUNCTION_ID,
    RUN_STATUS_LATCH_PARAM_ID, RUN_STATUS_OUT_PARAM_ID,
};
use crate::schema::_RET_PARAM_ID;
use crate::{lower_behavior_tree, schema_groot, FunctionIndex, LoweredTree};
use arora_behavior::Status;

/// The name of the language this interpreter reads: the [`GraphType::name`] a
/// graph spawned on it states.
pub const GRAPH_TYPE_NAME: &str = "behavior-tree";

/// The version of the `behavior-tree` format this interpreter reads. A graph
/// written for it, or for an older minor version of its major, runs.
///
/// - `1.0`: the control nodes, the status leaves, the task-run nodes and
///   module function leaves;
/// - `1.1` adds the data nodes `Equal` and `Unpack`
///   ([`EQUAL_FUNCTION_ID`](crate::nodes::EQUAL_FUNCTION_ID),
///   [`UNPACK_FUNCTION_ID`](crate::nodes::UNPACK_FUNCTION_ID)).
pub const GRAPH_TYPE_VERSION: semver::Version = semver::Version::new(1, 1, 0);

/// The [`GraphType`] this interpreter reads: [`GRAPH_TYPE_NAME`] at
/// [`GRAPH_TYPE_VERSION`]. A graph states the oldest version that has every
/// node it uses, so that an interpreter reading an older minor still runs it:
/// a graph without the `1.1` data nodes is spawned as `1.0`.
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
    /// The variables the run's spawn declared: they leave with the run when
    /// no remaining link reads them.
    variables: Vec<Uuid>,
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
    /// The nodes the graph's changes since the last lowering added, replaced
    /// or relinked: they start afresh at the next lowering, and every other
    /// node keeps its state.
    fresh: HashSet<Uuid>,
}

/// Bind a `{var}` name to the store slot under that name — the Direct
/// convention (variable name == store key). The lowering never asks it for a
/// tree-local name ([`is_tree_local`](crate::variable::is_tree_local)).
fn direct_resolver(store: &dyn DataStore) -> impl Fn(&str) -> Option<Box<dyn Slot>> + '_ {
    move |name: &str| Some(store.slot(&Key::from(name)))
}

/// `from` and the nodes under it in `graph`, through the children relation.
fn subtree(graph: &Graph, from: Uuid) -> HashSet<Uuid> {
    reach(graph, [from])
}

/// The variable a link source reads, through any [`LinkSource::Select`].
fn source_variable(source: &LinkSource) -> Option<Uuid> {
    match source {
        LinkSource::Variable(id) => Some(*id),
        LinkSource::Select { source, .. } => source_variable(source),
        _ => None,
    }
}

/// Check that `graph` is one tree rooted at `root`: every child is one of its
/// nodes, no node has two parents or is the root's parent, and every node is
/// reached from the root.
fn check_tree(graph: &Graph, root: Uuid) -> Result<(), BehaviorError> {
    let mut parent: HashMap<Uuid, Uuid> = HashMap::new();
    for node in graph.nodes.values() {
        for child in node.children.iter().flatten() {
            if !graph.nodes.contains_key(child) {
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
    if let Some(stray) = graph
        .nodes
        .keys()
        .find(|id| **id != root && !parent.contains_key(id))
    {
        return Err(behavior_error(format!(
            "node {stray} is not under the graph's root {root}"
        )));
    }
    // One parent each and a parentless root: a cycle would leave its nodes
    // unreachable from the root.
    if subtree(graph, root).len() != graph.nodes.len() {
        return Err(behavior_error("the graph's children form a cycle"));
    }
    Ok(())
}

/// Check that the nodes under `root` name only nodes as children and hold no
/// cycle. A node may sit under several parents.
fn check_acyclic(graph: &Graph, root: Uuid) -> Result<(), BehaviorError> {
    // Depth-first, each node once: `open` holds the path being walked, so a
    // child on it closes a cycle.
    let mut done = HashSet::new();
    let mut open = HashSet::from([root]);
    let mut stack = vec![(root, 0usize)];
    while let Some((id, next)) = stack.last_mut() {
        let children = graph.nodes[id].children.as_deref().unwrap_or_default();
        let Some(child) = children.get(*next).copied() else {
            open.remove(id);
            done.insert(*id);
            stack.pop();
            continue;
        };
        *next += 1;
        if !graph.nodes.contains_key(&child) {
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

/// Check that no selection is written: a `Select` link reads part of its
/// source, so it cannot feed a node's return (`_ret`) or a parameter its
/// function declares mutable, whose writes would land on the whole source.
fn check_selections(graph: &Graph, index: &FunctionIndex) -> Result<(), BehaviorError> {
    for link in &graph.links {
        if !matches!(link.source, LinkSource::Select { .. }) {
            continue;
        }
        let Port { node, port } = link.target;
        let written = port == _RET_PARAM_ID
            || graph
                .nodes
                .get(&node)
                .and_then(|node| index.get(&node.function))
                .and_then(|function| function.function.parameters.get(&port))
                .is_some_and(|parameter| parameter.mutable);
        if written {
            return Err(behavior_error(format!(
                "the selection linked to {node}.{port} would be written: a selection is \
                 read-only, so link a variable there"
            )));
        }
    }
    Ok(())
}

/// The nodes `starts` reach through the children relation, `starts`
/// included.
fn reach(graph: &Graph, starts: impl IntoIterator<Item = Uuid>) -> HashSet<Uuid> {
    let mut found = HashSet::new();
    let mut pending: Vec<Uuid> = starts.into_iter().collect();
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        let children = graph.nodes.get(&id).and_then(|node| node.children.as_ref());
        pending.extend(children.into_iter().flatten());
    }
    found
}

/// The nodes `diff` changes but does not remove: those it adds or replaces,
/// the targets of the links it adds or removes, and the nodes whose
/// predetermined keys it sets. Every field of a diff is named, so a new one
/// must be placed here.
fn changed_nodes(diff: &GraphDiff) -> impl Iterator<Item = Uuid> + '_ {
    let GraphDiff {
        add_nodes,
        remove_nodes: _,
        add_links,
        remove_links,
        set_predetermined,
        set_root: _,
        variables: _,
    } = diff;
    add_nodes
        .iter()
        .map(|node| node.id)
        .chain(add_links.iter().map(|link| link.target.node))
        .chain(remove_links.iter().map(|port| port.node))
        .chain(set_predetermined.iter().map(|(port, _)| port.node))
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
            main_root: None,
            runs: Vec::new(),
            finished: Vec::new(),
            halted: Vec::new(),
            lowered: None,
            dirty: true,
            fresh: HashSet::new(),
        }
    }

    /// Load a behavior tree from Groot XML, replacing the main behavior. The
    /// XML lowers onto the shared [`Graph`] (names → arora ids, resolved
    /// against `functions`; `{var}` → named variables) and grafts through
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

    /// The root of `run`'s program: its decorator's one child.
    fn root_of(&self, run: &Run) -> Option<Uuid> {
        self.graph.nodes[&run.decorator]
            .children
            .as_deref()
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

    /// Every run's nodes: each decorator and its subtree.
    fn run_nodes(&self) -> HashSet<Uuid> {
        self.all_runs()
            .flat_map(|run| subtree(&self.graph, run.decorator))
            .collect()
    }

    /// The variables a link into a run's nodes reads.
    fn run_variables(&self) -> HashSet<Uuid> {
        let nodes = self.run_nodes();
        self.graph
            .links
            .iter()
            .filter(|link| nodes.contains(&link.target.node))
            .filter_map(|link| source_variable(&link.source))
            .collect()
    }

    /// Refuse a declaration that renames one of `in_use`.
    fn check_renames(
        &self,
        variables: &HashMap<Uuid, String>,
        in_use: &HashSet<Uuid>,
        whose: &str,
    ) -> Result<(), BehaviorError> {
        for (id, name) in variables {
            match self.graph.variables.get(id) {
                Some(declared) if declared != name && in_use.contains(id) => {
                    return Err(behavior_error(format!(
                        "variable {id} is declared as '{declared}' and used by {whose}: it \
                         cannot become '{name}'"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Apply `diff` to the scaffold graph and mark it for re-lowering. The
    /// diff applies whole or not at all: it is refused when it fails, when the
    /// tree under the runner would name a child that is not a node or hold a
    /// cycle, when a selection would feed a return binding, or when the graph
    /// would not lower. A selection feeding a parameter the function declares
    /// mutable fails the next tick, which holds the device's index.
    fn edit(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        let changed: Vec<Uuid> = changed_nodes(&diff).collect();
        let mut graph = self.graph.clone();
        graph
            .apply(diff)
            .map_err(|e| behavior_error(format!("graph diff: {e}")))?;
        check_acyclic(&graph, self.runner)?;
        // Whether a parameter is mutable is the device's index's to say, which
        // the tick hands over: here only the return binding is checked, and
        // the lowering checks the rest.
        check_selections(&graph, &FunctionIndex::new())?;
        build_behavior_tree(&graph, &|_| None)
            .map_err(|e| behavior_error(format!("the edited graph does not lower: {e:?}")))?;
        self.graph = graph;
        self.dirty = true;
        self.fresh.extend(changed);
        Ok(())
    }

    /// Graft a run under the runner: a run-status decorator over `root`, with
    /// `fragment` — the run's nodes and links — added beside it. The run's keys
    /// live under `arora/tasks/<module>/<function>/<run>/`.
    fn graft(
        &mut self,
        module: &str,
        function: Uuid,
        root: Uuid,
        mut fragment: GraphDiff,
    ) -> Result<TaskHandle, BehaviorError> {
        let task = TaskId(Uuid::new_v4());
        let prefix = format!("arora/tasks/{module}/{function}/{}", task.0);
        let status_key = Key::from(format!("{prefix}/status"));
        let variables = fragment.variables.keys().copied().collect();

        // The decorator's status output is predetermined to the run's status
        // key (the Direct convention binds it to the store); its latch starts
        // empty.
        let decorator = Uuid::new_v4();
        fragment.add_nodes.push(GraphNode {
            id: decorator,
            function: RUN_STATUS_FUNCTION_ID,
            inputs: vec![Io::new(RUN_STATUS_LATCH_PARAM_ID)],
            outputs: vec![Io {
                predetermined_key: Some(status_key.path.clone()),
                ..Io::new(RUN_STATUS_OUT_PARAM_ID)
            }],
            children: Some(vec![root]),
        });
        fragment.add_links.push(Link::new(
            Port::new(decorator, RUN_STATUS_LATCH_PARAM_ID),
            LinkSource::Literal(Value::Unit),
        ));
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
            variables,
        });
        fragment.add_nodes.push(self.runner_node());
        self.edit(fragment).inspect_err(|_| {
            self.runs.pop();
        })?;
        Ok(handle)
    }

    /// The nodes something besides `leaving` holds, with what they reach: the
    /// main behavior, from its root even when `leaving` holds that too, and
    /// every node `leaving` does not have.
    fn held_outside(&self, leaving: &HashSet<Uuid>) -> HashSet<Uuid> {
        reach(
            &self.graph,
            self.graph
                .nodes
                .keys()
                .filter(|id| **id != self.runner && !leaving.contains(id))
                .copied()
                .chain(self.main_root),
        )
    }

    /// Bring the live run `self.runs[at]`, whose root is `graph`'s, to run
    /// `graph`, and answer the run's handle. Only what differs changes: the
    /// graph's nodes the run lacks or holds otherwise, the links into them,
    /// the run's nodes the graph does not have (but for those another node
    /// also holds), and the variables the graph declares anew. A graph the run
    /// already runs changes nothing.
    fn respawn(&mut self, at: usize, graph: Graph) -> Result<TaskHandle, BehaviorError> {
        let decorator = self.runs[at].decorator;
        let mut held = subtree(&self.graph, decorator);
        held.remove(&decorator);
        if let Some(id) = graph
            .nodes
            .keys()
            .find(|id| self.graph.nodes.contains_key(id) && !held.contains(id))
        {
            return Err(behavior_error(format!(
                "node {id} is already part of the device's behavior"
            )));
        }
        let declared = self.graph.variables.keys().copied().collect();
        self.check_renames(&graph.variables, &declared, "the device's behavior")?;

        let links: HashMap<Port, &LinkSource> = self
            .graph
            .links
            .iter()
            .map(|link| (link.target, &link.source))
            .collect();
        let linked: HashSet<Port> = graph.links.iter().map(|link| link.target).collect();
        let mut leaving = held.clone();
        leaving.insert(decorator);
        let staying = self.held_outside(&leaving);
        let diff = GraphDiff {
            remove_nodes: held
                .iter()
                .filter(|id| !graph.nodes.contains_key(id) && !staying.contains(id))
                .copied()
                .collect(),
            remove_links: links
                .keys()
                .filter(|port| graph.nodes.contains_key(&port.node) && !linked.contains(port))
                .copied()
                .collect(),
            add_nodes: graph
                .nodes
                .values()
                .filter(|node| self.graph.nodes.get(&node.id) != Some(*node))
                .cloned()
                .collect(),
            add_links: graph
                .links
                .iter()
                .filter(|link| links.get(&link.target) != Some(&&link.source))
                .cloned()
                .collect(),
            variables: graph
                .variables
                .iter()
                .filter(|(id, name)| self.graph.variables.get(*id) != Some(*name))
                .map(|(id, name)| (*id, name.clone()))
                .collect(),
            ..GraphDiff::default()
        };
        if diff != GraphDiff::default() {
            self.edit(diff)?;
        }
        let run = &mut self.runs[at];
        for id in graph.variables.keys() {
            if !run.variables.contains(id) {
                run.variables.push(*id);
            }
        }
        Ok(run.handle.clone())
    }

    /// Prune `runs`, already taken out of the live and finished lists: each
    /// decorator and its subtree leave the graph, but for the nodes another
    /// node — of the main behavior or of a remaining run — also holds; then the
    /// variables a run declared that no remaining link reads.
    fn prune(&mut self, runs: Vec<Run>) -> Result<(), BehaviorError> {
        if runs.is_empty() {
            return Ok(());
        }
        let leaving = reach(&self.graph, runs.iter().map(|run| run.decorator));
        let staying = self.held_outside(&leaving);
        let diff = GraphDiff {
            remove_nodes: leaving.difference(&staying).copied().collect(),
            add_nodes: vec![self.runner_node()],
            ..GraphDiff::default()
        };
        self.edit(diff)?;

        let read: HashSet<Uuid> = self
            .graph
            .links
            .iter()
            .filter_map(|link| source_variable(&link.source))
            .collect();
        for id in runs.iter().flat_map(|run| &run.variables) {
            if !read.contains(id) {
                self.graph.variables.remove(id);
            }
        }
        Ok(())
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
        // The halted runs' terminal status first: this tick owns the store.
        self.write_halts(ctx.store)?;

        // The graph changed: replace the lowering. Finished fragments leave
        // first. The new tree takes over the old one's state but for the
        // fresh nodes, then the old registration is undone. A lowering error
        // is transient — the graph stays, the next tick retries — surfaced
        // like any failed tick.
        if self.dirty {
            self.prune_finished()?;
            check_selections(&self.graph, ctx.functions)?;
            let tree = build_behavior_tree(&self.graph, &direct_resolver(ctx.store))
                .map_err(|e| behavior_error(format!("lowering the scaffold: {e:?}")))?;
            if let Some(lowered) = self.lowered.take() {
                tree.keep_state(lowered.tree(), &self.fresh);
                lowered.unregister(ctx.call_bridge);
            }
            self.fresh.clear();
            self.lowered = Some(
                lower_behavior_tree(tree, ctx.functions, ctx.call_bridge, false)
                    .map_err(|e| behavior_error(format!("registering the scaffold: {e:?}")))?,
            );
            self.dirty = false;
        }

        // One runtime tick is one tree tick. The runner's own status is not an
        // interpreter outcome — the scaffold is a standing policy, so the
        // interpreter stays `Running` (it is never dropped).
        if let Some(lowered) = &self.lowered {
            lowered
                .tick(ctx.call_bridge)
                .map_err(|e| behavior_error(format!("behavior tree: {e:?}")))?;
        }

        // Runs that reached a terminal status this tick latch and await
        // pruning; their ids stop resolving (halting them is now a no-op).
        self.sweep_terminal_runs(ctx.store);

        Ok(BehaviorStatus::Running)
    }

    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        // An editor's edit, onto the scaffold. It keeps the scaffold's
        // invariants — the root is always the runner, and every run hangs from
        // its decorator:
        // - `set_root` means "the main behavior's root is now X" — the runner
        //   re-parents it rather than being dethroned;
        // - the runner cannot be removed;
        // - an edit that changes a decorator, or removes a run's root, is
        //   refused: halting is how a run leaves.
        self.prune_finished()?;
        let mut diff = diff;
        let scaffold = |id: &Uuid| *id == self.runner || self.decorated_by(id).is_some();
        if let Some(id) = diff
            .set_root
            .iter()
            .chain(
                diff.add_nodes
                    .iter()
                    .flat_map(|node| node.children.iter().flatten()),
            )
            .find(|id| scaffold(id))
        {
            return Err(behavior_error(format!(
                "node {id} is the runner or a task run's decorator: an edit cannot place it"
            )));
        }
        let touched = changed_nodes(&diff).chain(diff.remove_nodes.iter().copied());
        for id in touched {
            if let Some(run) = self.decorated_by(&id) {
                return Err(behavior_error(format!(
                    "node {id} is the decorator of task run {}: an edit cannot change it",
                    run.handle.id.0
                )));
            }
        }
        for run in self.all_runs() {
            let roots = self.graph.nodes[&run.decorator].children.iter().flatten();
            if let Some(root) = roots.into_iter().find(|root| {
                diff.remove_nodes.contains(root) && !diff.add_nodes.iter().any(|n| n.id == **root)
            }) {
                return Err(behavior_error(format!(
                    "node {root} is the root of task run {}: halt the run to remove it",
                    run.handle.id.0
                )));
            }
        }
        self.check_renames(&diff.variables, &self.run_variables(), "a task run")?;

        diff.remove_nodes.retain(|id| *id != self.runner);
        if self.main_root.is_some_and(|root| {
            diff.remove_nodes.contains(&root) && !diff.add_nodes.iter().any(|n| n.id == root)
        }) {
            self.main_root = None;
        }
        if let Some(root) = diff.set_root.take() {
            self.main_root = Some(root);
        }
        // A fresh main root (or a removed one) re-parents under the runner.
        diff.add_nodes.push(self.runner_node());
        self.edit(diff)
    }

    fn load(&mut self, graph: Graph) -> Result<(), BehaviorError> {
        // A whole-behavior replacement: the previous main behavior — every node
        // that is not the scaffold's or a run's — leaves the scaffold, the new
        // behavior grafts under the runner. Live task runs are untouched — they
        // belong to their callers, not to the behavior — so the new behavior
        // may not reuse their nodes or rename their variables.
        self.prune_finished()?;
        let run_nodes = self.run_nodes();
        if let Some(id) = graph
            .nodes
            .keys()
            .find(|id| **id == self.runner || run_nodes.contains(id))
        {
            return Err(behavior_error(format!(
                "node {id} belongs to the runner or a task run: a loaded behavior cannot \
                 replace it"
            )));
        }
        self.check_renames(&graph.variables, &self.run_variables(), "a task run")?;
        let remove_nodes = self
            .graph
            .nodes
            .keys()
            .filter(|id| **id != self.runner && !run_nodes.contains(id))
            .copied()
            .collect();
        let mut diff = GraphDiff {
            remove_nodes,
            ..GraphDiff::default()
        };
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
        let module = call
            .module_id
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".to_string());
        let function = call.id;

        // The run is one run-call leaf; the call is literal link data.
        let call_value = value_serde::to_value(&call).map_err(|e| {
            behavior_error(format!("the spawned call does not convert to a value: {e}"))
        })?;
        self.prune_finished()?;
        let call_node = Uuid::new_v4();
        let fragment = GraphDiff {
            add_nodes: vec![GraphNode {
                id: call_node,
                function: RUN_CALL_FUNCTION_ID,
                inputs: vec![Io::new(RUN_CALL_PARAM_ID)],
                ..GraphNode::default()
            }],
            add_links: vec![Link::new(
                Port::new(call_node, RUN_CALL_PARAM_ID),
                LinkSource::Literal(call_value),
            )],
            ..GraphDiff::default()
        };
        self.graft(&module, function, call_node, fragment)
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
        if !graph.nodes.contains_key(&root) {
            return Err(behavior_error(format!(
                "the spawned graph's root {root} is not one of its nodes"
            )));
        }
        // The run is its own region of the scaffold: one tree of nodes the
        // device does not have (or its own, for a live run's root), of
        // functions the interpreter runs, whose links stay among them and
        // lower, renaming no declared variable.
        check_tree(&graph, root)?;
        if let Some(node) = graph
            .nodes
            .values()
            .find(|node| is_interpreter_function_id(node.function))
        {
            return Err(behavior_error(format!(
                "node {} calls the interpreter module's function {}, which a tree cannot call",
                node.id, node.function
            )));
        }
        if let Some(link) = graph.links.iter().find(|link| {
            !graph.nodes.contains_key(&link.target.node)
                || source_port(&link.source)
                    .is_some_and(|port| !graph.nodes.contains_key(&port.node))
        }) {
            return Err(behavior_error(format!(
                "the spawned graph's link onto {}.{} reaches outside its nodes",
                link.target.node, link.target.port
            )));
        }
        build_behavior_tree(&graph, &|_| None)
            .map_err(|e| behavior_error(format!("the spawned graph does not lower: {e:?}")))?;
        self.prune_finished()?;
        // A run is known by its root: the live run whose root this is runs
        // the graph from now on.
        if let Some(at) = self
            .runs
            .iter()
            .position(|run| self.root_of(run) == Some(root))
        {
            return self.respawn(at, graph);
        }
        if let Some(id) = graph
            .nodes
            .keys()
            .find(|id| self.graph.nodes.contains_key(id))
        {
            return Err(behavior_error(format!(
                "node {id} is already part of the device's behavior"
            )));
        }
        let declared = self.graph.variables.keys().copied().collect();
        self.check_renames(&graph.variables, &declared, "the device's behavior")?;
        let load = GraphDiff::load(graph);
        let fragment = GraphDiff {
            add_nodes: load.add_nodes,
            add_links: load.add_links,
            variables: load.variables,
            ..GraphDiff::default()
        };
        self.graft(
            &interpreter_module::ID.to_string(),
            interpreter_module::SPAWN_GRAPH,
            root,
            fragment,
        )
    }

    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
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
