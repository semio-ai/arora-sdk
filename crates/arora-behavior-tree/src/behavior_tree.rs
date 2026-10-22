pub mod behavior;
#[cfg(test)]
mod call_expression_tests;
pub mod core_nodes;
pub mod detached;
#[cfg(test)]
mod detached_tests;
pub mod display;
pub mod error;
pub mod graph;
mod id_hash;
mod incremental;
#[cfg(test)]
mod incremental_tests;
pub mod nodes;
mod scatter;
pub mod schema;
pub mod schema_groot;
#[cfg(test)]
mod tests;
pub mod tree_node;
pub mod variable;
pub use arora_behavior::{FunctionIndex, ModuleFunction};
use detached::Detached;
pub use detached::DetachedPlan;

use crate::display::{stated_id, Names};
use crate::variable::{is_tree_local, VariableCell, VariableResolver};
use arora_behavior::Status;
use arora_behavior_tree_types::TickId;
use arora_types::call::{CallBridge, CallError, Callable, CallableId};
use arora_types::record::module::frozen::Function;
use arora_types::AroraType;
use arora_types::{
    call::Call,
    value::{ConversionError, StructureField, Value},
};
use error::BehaviorTreeError;
use schema::{
    CallExpression, Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use uuid::Uuid;

use crate::nodes::{
    EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, FAIL_FUNCTION_ID, FALLBACK_FUNCTION_ID,
    PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID, RUN_CALL_PARAM_ID, RUN_FUNCTION_ID,
    RUN_STATUS_FUNCTION_ID, RUN_STATUS_LATCH_PARAM_ID, RUN_STATUS_OUT_PARAM_ID,
    SCATTER_FUNCTION_ID, SCATTER_KEYS_PARAM_ID, SCATTER_VALUES_PARAM_ID, SEQ_FUNCTION_ID,
    SEQ_STAR_CURRENT_INDEX_PARAM_ID, SEQ_STAR_FUNCTION_ID, SUCCEED_FUNCTION_ID, UNPACK_FUNCTION_ID,
    UNPACK_OUT_PARAM_IDS, UNPACK_VALUES_PARAM_ID,
};

// Runtime.
//====================================================================
/// The behavior tree, binding all nodes, variables and types together.
pub struct BehaviorTree {
    /// The root node from which the tree stems.
    root: Rc<Node>,
    /// All the nodes, indexed by their ID.
    node_index: HashMap<Uuid, Rc<Node>>,
    /// The local variables.
    variables: Rc<RefCell<HashMap<Uuid, VariableCell>>>,
    /// Variables associated to node arguments (node, arg).
    node_arg_variables: Rc<crate::id_hash::IdMap<NodeParameterId, VariableCell>>,
    /// Each `Unpack` and `Scatter` node's outputs, bound at load.
    outputs: HashMap<Uuid, Outputs>,
    /// The nodes no node holds, evaluated when what they output is read.
    detached: Rc<Detached>,
}

/// Where a data node writes, bound when the tree is loaded.
#[derive(Clone)]
enum Outputs {
    /// An `Unpack` node's output cells, by port index up to its last bound
    /// port: `None` for an unbound port.
    Ports(Rc<[Option<VariableCell>]>),
    /// A `Scatter` node's key prefix, and the slots its keys resolved to.
    Keys(Rc<scatter::Scatter>),
}

impl BehaviorTree {
    /// How the tree's detached nodes are evaluated, once it is lowered.
    pub fn detached_plan(&self) -> DetachedPlan {
        self.detached.plan()
    }

    /// How many times the detached node `id` has called its function.
    pub fn detached_calls(&self, id: Uuid) -> Option<u64> {
        self.detached.calls(id)
    }

    /// Observe the detached nodes among `ids`: from the next lowering on,
    /// each is brought up to date at every tick, after the tree ticks, as a
    /// sink is, whether or not a node reads it. A host showing what every
    /// node of a graph computes observes them all.
    pub fn observe_detached(&self, ids: impl IntoIterator<Item = Uuid>) {
        self.detached.observe(ids);
    }

    /// The outputs of the detached node `id` after its last call, by port:
    /// its return value under [`_RET_PARAM_ID`], and each mutable parameter
    /// under its id. `None` for a node that is not detached or has not been
    /// called yet.
    pub fn detached_outputs(&self, id: Uuid) -> Option<HashMap<Uuid, Value>> {
        self.detached.outputs(id)
    }

    /// The output `port` of the detached node `id` after its last call: its
    /// return value for [`_RET_PARAM_ID`]. `None` as for
    /// [`detached_outputs`](Self::detached_outputs), or for a port the node
    /// has not output.
    pub fn detached_output(&self, id: Uuid, port: Uuid) -> Option<Value> {
        self.detached.output(id, port)
    }

    /// The value the cell of `node`'s argument `parameter` holds: what a node
    /// reading `@node.parameter` reads. A detached node's argument bound to a
    /// store key has a cell of its own, apart from the key: the port the key
    /// is published from. `None` for an argument the tree does not have, or a
    /// cell that holds nothing.
    pub fn argument_value(&self, node: Uuid, parameter: Uuid) -> Option<Value> {
        self.node_arg_variables
            .get(&NodeParameterId { node, parameter })
            .and_then(VariableCell::get)
    }

    /// Set the cell of `node`'s argument `parameter` to `value`, as a write
    /// of the node would: a host restoring a node's state into a tree built
    /// again from a changed node. Returns whether the tree has the argument.
    pub fn set_argument_value(&self, node: Uuid, parameter: Uuid, value: Option<Value>) -> bool {
        match self
            .node_arg_variables
            .get(&NodeParameterId { node, parameter })
        {
            Some(cell) => {
                cell.set_opt(value);
                true
            }
            None => false,
        }
    }

    /// The tree's detached nodes as text, one per line, in the order the tree
    /// lists them.
    pub fn display_detached(&self, names: Names) -> String {
        self.detached
            .nodes()
            .iter()
            .map(|node| format!("{}\n", node.display(names)))
            .collect()
    }

    /// Find who reads which detached node, against `functions`. The attached
    /// nodes are the root and the nodes it holds: a loose node that is not
    /// detached is inert, and writes nothing.
    fn lower_detached(&self, functions: &FunctionIndex) -> Result<(), BehaviorTreeError> {
        let mut attached: Vec<Rc<Node>> = Vec::new();
        let mut seen = HashSet::new();
        let mut held = vec![self.root.clone()];
        while let Some(node) = held.pop() {
            if seen.insert(node.id) {
                held.extend(
                    node.children
                        .iter()
                        .flatten()
                        .filter_map(|child| self.node_index.get(child).cloned()),
                );
                attached.push(node);
            }
        }
        self.detached.lower(
            functions,
            &attached,
            &self.variables.borrow(),
            &self.node_arg_variables,
        )
    }

    /// Tick `root` once, its detached nodes around it: they are out of date
    /// until read, and the sinks are brought up to date and written after the
    /// tree has ticked, whether or not the tree failed. The tick reports the
    /// tree's error first, then the sinks'.
    fn tick_root(
        &self,
        root: &TickLink,
        caller: &mut dyn CallBridge,
    ) -> Result<Status, BehaviorTreeError> {
        let _store = root.function.incremental.begin_tick();
        self.detached.begin_tick();
        let status = root.tick(caller);
        let variables = self.variables.borrow();
        let calls = FunctionIndex::new();
        let sinks = self.detached.tick_sinks(
            Scope {
                variables: &variables,
                node_parameters_variables: &self.node_arg_variables,
                calls: &calls,
            },
            caller,
        );
        let status = status?;
        sinks?;
        Ok(status)
    }

    /// Take over the state `previous` holds for every node of this tree that
    /// `fresh` does not name: a tree built again from a changed graph goes on
    /// where the previous one was for the nodes the change left in place.
    ///
    /// A node's state is the tree-local cells its arguments hold — a literal
    /// argument's cell, which a node such as `seq_star` advances, and the
    /// tree-local variables it reads or writes. Each such cell takes the value
    /// it holds in `previous`, which gives up that value: `previous` is the
    /// tree being replaced. A cell bound to another node's argument is that
    /// node's to carry, and a store-backed cell holds nothing of the tree's. A
    /// tree-local variable that only fresh nodes read starts afresh with them.
    pub fn keep_state(&self, previous: &BehaviorTree, fresh: &HashSet<Uuid>) {
        let mut read = HashSet::new();
        for node in self.node_index.values() {
            if fresh.contains(&node.id) {
                continue;
            }
            for (parameter, expression) in &node.arguments {
                variable_ids_in(expression, &mut read);
                match selected_source(expression) {
                    Expression::VariableId(_) => {}
                    Expression::Value(_) | Expression::Uuid(_) | Expression::Call(_) => {
                        let at = NodeParameterId {
                            node: node.id,
                            parameter: *parameter,
                        };
                        if let Some(held) = previous
                            .node_index
                            .get(&node.id)
                            .and_then(|previous| previous.arguments.get(parameter))
                        {
                            take_call_site_cells(expression, held);
                        }
                        if let (Some(cell), Some(held)) = (
                            self.node_arg_variables.get(&at),
                            previous.node_arg_variables.get(&at),
                        ) {
                            take_local(cell, held);
                        }
                    }
                    // Another node's argument, or a cell the tree was built
                    // with: not this node's to carry. `selected_source` has
                    // unwrapped every selection.
                    Expression::NodeArgument(_)
                    | Expression::Variable(_)
                    | Expression::Select { .. } => {}
                }
            }
        }
        let (cells, held) = (self.variables.borrow(), previous.variables.borrow());
        for variable in read {
            if let (Some(cell), Some(held)) = (cells.get(&variable), held.get(&variable)) {
                take_local(cell, held);
            }
        }
    }
}

/// Every variable `expression` names by id, at any depth: a call's arguments
/// and a selection's source included.
fn variable_ids_in(expression: &Expression, found: &mut HashSet<Uuid>) {
    match expression {
        Expression::VariableId(id) => {
            found.insert(*id);
        }
        Expression::Select { source, .. } => variable_ids_in(source, found),
        Expression::Call(call) => {
            let parts = [&call.module, &call.function].into_iter().chain(
                call.arguments
                    .iter()
                    .flat_map(|(parameter, argument)| [parameter, argument]),
            );
            for part in parts {
                variable_ids_in(part, found);
            }
        }
        _ => {}
    }
}

/// The expression a selection reads, through any nesting; `expression`
/// itself when it is not a selection.
fn selected_source(expression: &Expression) -> &Expression {
    match expression {
        Expression::Select { source, .. } => selected_source(source),
        expression => expression,
    }
}

/// Move `held`'s value into `cell` when both are tree-local, leaving `held`
/// the value `cell` had.
fn take_local(cell: &VariableCell, held: &VariableCell) {
    if matches!(
        (cell, held),
        (VariableCell::Local(_), VariableCell::Local(_))
    ) {
        let value = cell.get();
        cell.set_opt(held.get());
        held.set_opt(value);
    }
}

pub struct BehaviorTreeRuntime<'a> {
    caller: &'a mut dyn CallBridge,
    tick: TickLink,
    tree: &'a BehaviorTree,
}

impl<'a> BehaviorTreeRuntime<'a> {
    /// Enable incremental ticking (off by default), with the rules documented
    /// on [`LoweredTree::set_incremental`]: argument-version invalidation,
    /// lazy store observation, polling from the previous tick's visits, and no
    /// write observation while disabled. A skipped node emits no trace line.
    /// Changing this flag invalidates the cache; a fresh lowering starts dirty.
    pub fn set_incremental(&self, enabled: bool) {
        self.tick.function.incremental.set_enabled(enabled);
    }

    pub fn setup(
        tree: &'a BehaviorTree,
        functions: &FunctionIndex,
        caller: &'a mut dyn CallBridge,
        trace: bool,
    ) -> Result<Self, BehaviorTreeError> {
        let (tick, _) = register_ticks(tree, functions, caller, trace)?;
        Ok(Self { caller, tick, tree })
    }

    pub fn tick(&mut self) -> Result<Status, BehaviorTreeError> {
        self.tree.tick_root(&self.tick, self.caller)
    }
}

/// A behavior tree lowered onto a caller: every node's tick function registered
/// as an engine callable, ready to tick — **one tick at a time** (one arora
/// runtime tick is one behavior-tree tick). Holds the tree (its variable cells
/// back the registered ticks) and the callable ids, so the registration can be
/// [`unregister`](Self::unregister)ed when the lowering is replaced — without
/// it, every re-lowering would leak its callables in the engine.
pub struct LoweredTree {
    root: TickLink,
    callables: Vec<CallableId>,
    /// Keeps the tree's shared cells alive for the registered tick functions.
    tree: BehaviorTree,
}

/// Lower `tree` onto `caller`: register every node's tick function and return
/// the ready-to-tick [`LoweredTree`].
///
/// Each node calling a module function resolves it against `functions` here,
/// once: the lowered tree calls the module the index names, with the
/// signature it describes, until the tree is lowered again. A node whose
/// function the index does not have fails when it ticks, with
/// [`CallError::FunctionNotFound`].
pub fn lower_behavior_tree(
    tree: BehaviorTree,
    functions: &FunctionIndex,
    caller: &mut dyn CallBridge,
    trace: bool,
) -> Result<LoweredTree, BehaviorTreeError> {
    let (root, callables) = register_ticks(&tree, functions, caller, trace)?;
    Ok(LoweredTree {
        root,
        callables,
        tree,
    })
}

impl LoweredTree {
    /// Enable incremental ticking (off by default). Clean children and a clean
    /// root return their cached status without engine dispatch or trace output.
    /// Only Running and non-cacheable nodes visited last tick are polled, along
    /// with parents of visited detached readers. A bypassed Running child drops
    /// out; its cached Running status cannot answer when reached again.
    /// Module leaves require literal `_on_change: true` to cache terminal
    /// results (see [`schema::_ON_CHANGE_PARAM_ID`]).
    ///
    /// Every argument except `_ret` contributes its read cells, including
    /// mutable parameters. A version change during a visit, whoever wrote it,
    /// dirties the node for the next tick. A write only to its `_ret` cell
    /// does not self-invalidate; foreign overwrites still invalidate its producer.
    /// Store reads are observed at tick start and lazily before skipping clean
    /// subtrees after possible store movement, never globally after each visit.
    /// Non-cacheable visits flag possible movement before running: a module
    /// composite's children observe its raw writes before answering from cache.
    /// Detached pulls precede a reader's clean check; sinks still run after the
    /// root. Later writes invalidate earlier readers for the next tick.
    /// With the flag off, subscriptions are dropped and cell writes do no value
    /// observation. Enabling subscribes reads and initializes last-seen values.
    /// Changing the flag invalidates statuses. No halt is introduced.
    pub fn set_incremental(&self, enabled: bool) {
        self.root.function.incremental.set_enabled(enabled);
    }

    /// Tick the tree **once**: every reactive pass drills through the ready
    /// children and returns the root's status — [`Status::Running`] means "see
    /// you next tick", never "spin".
    pub fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        self.tree.tick_root(&self.root, caller)
    }

    /// The lowered tree, for callers that inspect its cells.
    pub fn tree(&self) -> &BehaviorTree {
        &self.tree
    }

    /// Unregister every callable this lowering registered. Call when replacing
    /// the lowering (an edit re-lowers); the engine forgets the old ticks.
    pub fn unregister(&self, caller: &mut dyn CallBridge) {
        for callable in &self.callables {
            caller.arora_unregister_callable(callable);
        }
    }
}

/// Lower `tree`'s detached nodes and register every attached node's tick
/// function on `caller`: the root's tick, and every callable registered. A
/// tree that does not lower leaves nothing registered: the callables
/// registered before the refusal are unregistered.
fn register_ticks(
    tree: &BehaviorTree,
    functions: &FunctionIndex,
    caller: &mut dyn CallBridge,
    trace: bool,
) -> Result<(TickLink, Vec<CallableId>), BehaviorTreeError> {
    tree.lower_detached(functions)?;
    let mut callables = Vec::new();
    let incremental = Rc::new(incremental::Incremental::default());
    #[cfg(test)]
    incremental.set_enabled(incremental_tests::existing_tests_enabled());
    let root = setup_tick_function(
        tree.root.clone(),
        &tree.node_index,
        functions,
        tree.variables.clone(),
        tree.node_arg_variables.clone(),
        tree.detached.clone(),
        &tree.outputs,
        caller,
        if trace {
            TraceTick::YesAll
        } else {
            TraceTick::No
        },
        &mut callables,
        incremental,
        None,
    );
    match root {
        Ok(root) => Ok((root, callables)),
        Err(error) => {
            for callable in &callables {
                caller.arora_unregister_callable(callable);
            }
            Err(error)
        }
    }
}

/// Runs a behavior tree until it reaches the status success or failure.
///
/// A convenience for trees that terminate (tests, scripts): the reactive model
/// is one tick per runtime tick ([`LoweredTree::tick`]); this drains ticks
/// back-to-back instead, so a tree waiting on outside state (e.g. the `run`
/// builtin) would spin forever here.
pub fn run_behavior_tree(
    behavior: &BehaviorTree,
    functions: &FunctionIndex,
    caller: &mut dyn CallBridge,
    trace: bool,
) -> Result<Status, BehaviorTreeError> {
    let mut runtime = BehaviorTreeRuntime::setup(behavior, functions, caller, trace)?;
    let mut status = Status::Running;
    while status == Status::Running {
        status = runtime.tick()?;
    }
    Ok(status)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TraceTick {
    YesAll,
    No,
}

#[allow(clippy::too_many_arguments)]
fn setup_tick_function(
    node: Rc<Node>,
    node_index: &HashMap<Uuid, Rc<Node>>,
    functions: &FunctionIndex,
    variables: Rc<RefCell<HashMap<Uuid, VariableCell>>>,
    node_arg_variables: Rc<crate::id_hash::IdMap<NodeParameterId, VariableCell>>,
    detached: Rc<Detached>,
    outputs: &HashMap<Uuid, Outputs>,
    caller: &mut dyn CallBridge,
    trace: TraceTick,
    // Every callable registered on behalf of this (sub)tree, collected so the
    // registration can be undone when the lowering is replaced.
    callables: &mut Vec<CallableId>,
    incremental: Rc<incremental::Incremental>,
    parent: Option<std::rc::Weak<incremental::State>>,
) -> Result<TickLink, BehaviorTreeError> {
    let pulls = detached.pulls(&node.id);
    let mut reads = Vec::new();
    let mut arguments = Vec::new();
    for (parameter, expression) in &node.arguments {
        let mut cells = Vec::new();
        detached::cells_read(
            expression,
            &variables.borrow(),
            &node_arg_variables,
            &mut cells,
        );
        cells.extend(
            node_arg_variables
                .get(&NodeParameterId {
                    node: node.id,
                    parameter: *parameter,
                })
                .cloned(),
        );
        if *parameter != _RET_PARAM_ID {
            arguments.extend(cells.iter().cloned());
        }
        reads.extend(cells);
    }
    let mut seen = HashSet::new();
    reads.retain(|cell| seen.insert(cell.identity()));
    seen.clear();
    arguments.retain(|cell| seen.insert(cell.identity()));
    let stored: Vec<_> = reads
        .iter()
        .filter(|cell| matches!(cell, VariableCell::Stored(_)))
        .cloned()
        .collect();
    let state = Rc::new(incremental::State {
        dirty: std::cell::Cell::new(true),
        status: std::cell::Cell::new(None),
        parent,
        cacheable: incremental::cacheable(&node),
        pulls: pulls.is_some(),
        reads,
        arguments,
        subtree_stored: RefCell::new(stored.clone()),
    });
    incremental.states.borrow_mut().push(Rc::downgrade(&state));
    for cell in &state.reads {
        if incremental.enabled.get() {
            cell.subscribe(&state);
        }
    }
    for cell in stored {
        let mut stored = incremental.stored.borrow_mut();
        if !stored.iter().any(|held| held.identity() == cell.identity()) {
            stored.push(cell);
        }
    }
    let nof_children = node
        .children
        .as_ref()
        .map(|children| children.len())
        .unwrap_or(0);
    let mut children_ticks: Vec<TickLink> = Vec::with_capacity(nof_children);
    if let Some(children) = &node.children {
        for child_id in children {
            let child_node = node_index
                .get(child_id)
                .ok_or(BehaviorTreeError::ChildNodeNotFound {
                    node: node.id,
                    child: *child_id,
                })?
                .clone();
            let tick_function_with_id = setup_tick_function(
                child_node.clone(),
                node_index,
                functions,
                variables.clone(),
                node_arg_variables.clone(),
                detached.clone(),
                outputs,
                caller,
                trace,
                callables,
                incremental.clone(),
                Some(Rc::downgrade(&state)),
            )?;
            children_ticks.push(tick_function_with_id);
        }
    }
    let mut seen: HashSet<_> = state
        .subtree_stored
        .borrow()
        .iter()
        .map(VariableCell::identity)
        .collect();
    for child in &children_ticks {
        for cell in child.function.state.subtree_stored.borrow().iter() {
            if seen.insert(cell.identity()) {
                state.subtree_stored.borrow_mut().push(cell.clone());
            }
        }
    }
    let tick_function = Rc::new(TickFunction {
        state,
        incremental,
        node: node.clone(),
        function: functions.get(&node.function).cloned(),
        calls: Rc::new(call_signatures(&node, functions)?),
        argument_order: crate::display::ordered(&node.arguments, functions.get(&node.function))
            .into_iter()
            .map(|(parameter, _)| parameter)
            .filter(|parameter| *parameter != _ON_CHANGE_PARAM_ID)
            .collect(),
        locals: variables.to_owned(),
        node_arg_variables: node_arg_variables.to_owned(),
        pulls,
        detached,
        outputs: outputs.get(&node.id).cloned(),
        children: children_ticks,
        trace,
    });
    let callable_id = caller.arora_register_callable(tick_function.clone());
    let tick = TickId {
        callable_id: callable_id.id,
    };
    callables.push(callable_id);
    Ok(TickLink {
        id: tick,
        function: tick_function,
    })
}

/// Whether `function` is a node this crate ticks natively — a control node, a
/// status leaf, a task-run node, a core string node or a data node — rather
/// than a module function the function index resolves.
pub(crate) fn is_native(function: Uuid) -> bool {
    matches!(
        function,
        SUCCEED_FUNCTION_ID
            | FAIL_FUNCTION_ID
            | RUN_FUNCTION_ID
            | SEQ_FUNCTION_ID
            | SEQ_STAR_FUNCTION_ID
            | FALLBACK_FUNCTION_ID
            | PARALLEL_FUNCTION_ID
            | RUN_CALL_FUNCTION_ID
            | RUN_STATUS_FUNCTION_ID
            | EQUAL_FUNCTION_ID
            | UNPACK_FUNCTION_ID
            | SCATTER_FUNCTION_ID
    ) || core_nodes::core_node_parameters(function).is_some()
}

/// Tick the native nodes (see [`is_native`]) — the control nodes, the status
/// leaves, the task-run nodes, the data nodes `Equal`, `Unpack` and `Scatter`
/// and the core string nodes ([`core_nodes`]) — without consulting the
/// function index or the wasm engine. Children are ticked through their registered tick functions
/// ([`Tickable for TickId`]). Returns `Some(status)` for a built-in node and
/// `None` for any other function (which the caller dispatches via the engine).
fn tick_builtin(
    caller: &mut dyn CallBridge,
    scope: Scope,
    outputs: Option<&Outputs>,
    child_tick_ids: &[TickLink],
    node: &Node,
) -> Result<Option<Status>, BehaviorTreeError> {
    // An argument's value: a call or a selection is computed, a selection
    // applying its path, and any other argument reads the argument's cell, as
    // a module leaf's does, which an output bound to this input writes. A
    // call's result is kept in the argument's cell, as a module leaf keeps
    // it, for a node that reads this argument.
    let argument = |caller: &mut dyn CallBridge, parameter: Uuid| {
        let node_parameter = NodeParameterId {
            node: node.id,
            parameter,
        };
        let expression = node.arguments.get(&parameter).ok_or_else(|| {
            BehaviorTreeError::InconsistentTreeError {
                message: format!("{node_parameter} has no argument"),
            }
        })?;
        let cell = scope.node_parameters_variables.get(&node_parameter);
        let value = match (expression, cell) {
            (Expression::Call(_) | Expression::Select { .. }, _) | (_, None) => {
                compute_expression(scope, expression, caller, &node_parameter)?
            }
            (_, Some(cell)) => cell.get_or_unit(),
        };
        if let (Expression::Call(_), Some(cell)) = (expression, cell) {
            cell.set_opt(Some(value.clone()).filter(|value| *value != Value::Unit));
        }
        Ok::<_, BehaviorTreeError>(value)
    };
    let status = match node.function {
        EQUAL_FUNCTION_ID => {
            if argument(caller, EQUAL_A_PARAM_ID)? == argument(caller, EQUAL_B_PARAM_ID)? {
                Status::Success
            } else {
                Status::Failure
            }
        }
        UNPACK_FUNCTION_ID => {
            let Some(Outputs::Ports(outputs)) = outputs else {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: format!("Unpack node {} has no bound outputs", node.id),
                });
            };
            match argument(caller, UNPACK_VALUES_PARAM_ID)?.into_elements() {
                Ok(values) if values.len() >= outputs.len() => {
                    // A slot notifies a change only when the value moves
                    // (the `Slot` contract), so a held value costs none.
                    for (cell, value) in outputs.iter().zip(values) {
                        if let Some(cell) = cell.as_ref().filter(|_| value != Value::Unit) {
                            cell.set(value);
                        }
                    }
                    Status::Success
                }
                _ => Status::Failure,
            }
        }
        SCATTER_FUNCTION_ID => {
            let Some(Outputs::Keys(scatter)) = outputs else {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: format!("Scatter node {} is not bound", node.id),
                });
            };
            scatter.write(
                argument(caller, SCATTER_KEYS_PARAM_ID)?,
                argument(caller, SCATTER_VALUES_PARAM_ID)?,
            )?
        }
        SUCCEED_FUNCTION_ID => Status::Success,
        FAIL_FUNCTION_ID => Status::Failure,
        RUN_FUNCTION_ID => Status::Running,
        SEQ_FUNCTION_ID => {
            let mut status = Status::Success;
            for child in child_tick_ids {
                match child.tick(caller)? {
                    Status::Success => continue,
                    Status::Failure => {
                        status = Status::Failure;
                        break;
                    }
                    Status::Running => {
                        status = Status::Running;
                        break;
                    }
                }
            }
            status
        }
        FALLBACK_FUNCTION_ID => {
            if child_tick_ids.is_empty() {
                Status::Success
            } else {
                let mut status = Status::Failure;
                for child in child_tick_ids {
                    match child.tick(caller)? {
                        Status::Success => {
                            status = Status::Success;
                            break;
                        }
                        Status::Failure => continue,
                        Status::Running => {
                            status = Status::Running;
                            break;
                        }
                    }
                }
                status
            }
        }
        PARALLEL_FUNCTION_ID => {
            // Ticks every child every time, then aggregates: Success only if
            // all children succeeded, Failure if any failed, else Running.
            let mut success = true;
            let mut failure = false;
            for child in child_tick_ids {
                match child.tick(caller)? {
                    Status::Success => continue,
                    Status::Failure => {
                        success = false;
                        failure = true;
                    }
                    Status::Running => {
                        success = false;
                    }
                }
            }
            if success {
                Status::Success
            } else if failure {
                Status::Failure
            } else {
                Status::Running
            }
        }
        RUN_CALL_FUNCTION_ID => {
            // A task-run call leaf: the full [`Call`] travels as one literal
            // parameter — introspectable data in the graph — and dispatches
            // straight through the caller, so any callable target runs (no
            // function-index entry required).
            let call_variable = get_node_parameter_variable(
                &NodeParameterId {
                    node: node.id,
                    parameter: RUN_CALL_PARAM_ID,
                },
                scope.node_parameters_variables,
            )?;
            let call: Call = arora_types::value_serde::from_value(call_variable.get_or_unit())
                .map_err(|e| BehaviorTreeError::InconsistentTreeError {
                    message: format!("run-call node {}: malformed call: {e}", node.id),
                })?;
            let result = caller
                .arora_call(call)
                .map_err(BehaviorTreeError::CallError)?;
            // A non-status return is a clean success: the call ran and produced
            // a value, and there is nothing to keep ticking for.
            result.ret.try_into().unwrap_or(Status::Success)
        }
        RUN_STATUS_FUNCTION_ID => {
            // The task-run status decorator: forwards its one child's status,
            // mirroring it onto the bound status output — a store key under the
            // Direct convention, where a run's observers watch it. Latches at
            // terminal: a finished run is inert (never re-invoked) until its
            // fragment is pruned. A child that errors ends the run `Failure`:
            // the status is written and latched, and the error goes up this
            // once, so it is reported and the run's siblings tick on from the
            // next tick.
            let latch = get_node_parameter_variable(
                &NodeParameterId {
                    node: node.id,
                    parameter: RUN_STATUS_LATCH_PARAM_ID,
                },
                scope.node_parameters_variables,
            )?;
            if let Ok(latched) = Status::try_from(latch.get_or_unit()) {
                if latched != Status::Running {
                    return Ok(Some(latched));
                }
            }
            let child = child_tick_ids
                .first()
                .ok_or(BehaviorTreeError::InconsistentTreeError {
                    message: format!("run-status decorator {} needs one child", node.id),
                })?;
            let ticked = child.tick(caller);
            let out = get_node_parameter_variable(
                &NodeParameterId {
                    node: node.id,
                    parameter: RUN_STATUS_OUT_PARAM_ID,
                },
                scope.node_parameters_variables,
            )?;
            let status = match ticked {
                Ok(status) => status,
                Err(error) => {
                    out.set(Status::Failure.into());
                    latch.set(Status::Failure.into());
                    return Err(error);
                }
            };
            out.set(status.into());
            if status != Status::Running {
                latch.set(status.into());
            }
            status
        }
        SEQ_STAR_FUNCTION_ID => {
            // The current index persists in the node's parameter variable, so
            // a Running tree resumes past the children that already succeeded.
            let index_variable = get_node_parameter_variable(
                &NodeParameterId {
                    node: node.id,
                    parameter: SEQ_STAR_CURRENT_INDEX_PARAM_ID,
                },
                scope.node_parameters_variables,
            )?;
            let mut current_index = match index_variable.get_or_unit() {
                Value::U16(index) => index,
                _ => 0,
            };
            let mut status = Status::Success;
            for child in child_tick_ids.iter().skip(current_index as usize) {
                match child.tick(caller)? {
                    Status::Success => current_index += 1,
                    Status::Failure => {
                        status = Status::Failure;
                        break;
                    }
                    Status::Running => {
                        status = Status::Running;
                        break;
                    }
                }
            }
            if status != Status::Running {
                current_index = 0;
            }
            index_variable.set(Value::U16(current_index));
            status
        }
        // The core string nodes, value-based over their bound cells. A call is
        // computed into its argument's cell first, in the node's parameter
        // order, as for every other node. A selection an input reads is
        // computed too, and read from a cell of its own; a parameter the node
        // writes keeps the cell it writes to.
        function => {
            let writes = detached::native_writes(function);
            let mut selected = HashMap::new();
            for parameter in core_nodes::core_node_parameters(function).unwrap_or_default() {
                match node.arguments.get(&parameter.id) {
                    Some(Expression::Call(_)) => {
                        argument(caller, parameter.id)?;
                    }
                    Some(Expression::Select { .. }) if !writes.contains(&parameter.id) => {
                        let value = argument(caller, parameter.id)?;
                        selected.insert(parameter.id, VariableCell::local_with(value));
                    }
                    _ => {}
                }
            }
            return core_nodes::tick_core_node(function, node.id, |parameter| {
                if let Some(cell) = selected.get(&parameter) {
                    return Ok(cell.clone());
                }
                get_node_parameter_variable(
                    &NodeParameterId {
                        node: node.id,
                        parameter,
                    },
                    scope.node_parameters_variables,
                )
                .cloned()
            });
        }
    };
    Ok(Some(status))
}

#[allow(clippy::too_many_arguments)]
fn tick(
    caller: &mut dyn CallBridge,
    function: Option<&ModuleFunction>,
    calls: &FunctionIndex,
    argument_order: &[Uuid],
    variables: Rc<RefCell<HashMap<Uuid, VariableCell>>>,
    node_parameters_variables: Rc<crate::id_hash::IdMap<NodeParameterId, VariableCell>>,
    detached: (&Detached, Option<&[usize]>),
    outputs: Option<&Outputs>,
    child_tick_ids: &[TickLink],
    node: Rc<Node>,
    trace: TraceTick,
) -> Result<Status, BehaviorTreeError> {
    let trace_line = |status: Status| {
        if trace != TraceTick::No {
            println!(
                "tick {} {} -> {:?}",
                node.id,
                node.display(Names::functions(calls)),
                status
            );
        }
    };
    // The detached nodes the node reads are brought up to date first.
    if let (detached, Some(pulls)) = detached {
        let _detached = incremental::Visit::detached();
        detached.pull(
            pulls,
            Scope {
                variables: &variables.borrow(),
                node_parameters_variables: &node_parameters_variables,
                calls,
            },
            caller,
        )?;
    }
    // The basic control nodes are wired in natively; everything else is
    // dispatched into a module through the engine.
    let builtin = tick_builtin(
        caller,
        Scope {
            variables: &variables.borrow(),
            node_parameters_variables: &node_parameters_variables,
            calls,
        },
        outputs,
        child_tick_ids,
        &node,
    )?;
    if let Some(status) = builtin {
        trace_line(status);
        return Ok(status);
    }

    let module_function =
        function.ok_or(BehaviorTreeError::CallError(CallError::FunctionNotFound {
            id: node.function,
        }))?;
    let function = &module_function.function;

    let mut call = Call {
        module_id: Some(module_function.module_id),
        id: node.function,
        args: Vec::with_capacity(
            node.arguments.len() + if node.children.is_some() { 1 } else { 0 },
        ),
    };

    let nof_children = node
        .children
        .as_ref()
        .map(|children| children.len())
        .unwrap_or(0);
    assert_eq!(nof_children, child_tick_ids.len());

    if node.children.is_some() {
        // Check the presence of the `children` parameter, which must be the first one.
        let first_parameter_id = function.parameter_ordering.first().ok_or(
            BehaviorTreeError::MissingChildrenParameter {
                node: node.id.to_owned(),
                function: node.function.to_owned(),
            },
        )?;
        let first_parameter = function.parameters.get(first_parameter_id).ok_or(
            BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            },
        )?;
        if first_parameter.name != "children" {
            Err(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?;
        }
        let first_parameter_type_id = first_parameter
            .ty
            .as_array()
            .ok_or(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?
            .reference
            .id;
        if first_parameter_type_id != TickId::arora_type_id() {
            Err(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?;
        }

        // Pass the tick ids of the children.
        call.args.push(StructureField {
            id: *first_parameter_id,
            value: Box::new(Value::array_of_type::<TickId>(
                child_tick_ids
                    .iter()
                    .map(|child| Value::from(child.id))
                    .collect(),
            )),
        })
    }

    // Resolving the remaining parameters, and passing them.
    // A local map of variables is maintained to update them when if mutated.
    // Some of them were setup to be shared, but it is transparent here.
    // They are indexed by parameter id.
    let mut locals = HashMap::<Uuid, VariableCell>::new();
    // Arguments read through a selection: the function receives the selected
    // part. A selection is read-only — the interpreter refuses one on a
    // mutable parameter — so a mutation reported for it is not written back
    // over the whole source.
    let mut selected = Vec::new();
    {
        let variables = variables.borrow();
        let scope = Scope {
            variables: &variables,
            node_parameters_variables: &node_parameters_variables,
            calls,
        };
        for param_id in argument_order {
            let value_expression = &node.arguments[param_id];
            let node_parameter = NodeParameterId {
                node: node.id.to_owned(),
                parameter: param_id.to_owned(),
            };
            let variable =
                get_node_parameter_variable(&node_parameter, &node_parameters_variables)?;

            // A call is computed into the argument's cell.
            if matches!(value_expression, Expression::Call(_)) {
                let value = compute_expression(scope, value_expression, caller, &node_parameter)?;
                variable.set_opt(Some(value).filter(|value| *value != Value::Unit));
            };

            if param_id == &_RET_PARAM_ID {
                locals.insert(*param_id, variable.clone());
                continue;
            }
            let value = if let Expression::Select { .. } = value_expression {
                selected.push(*param_id);
                // A selection over an absent value reads as absent (`Unit`).
                Some(compute_expression(
                    scope,
                    value_expression,
                    caller,
                    &node_parameter,
                )?)
                .filter(|value| *value != Value::Unit)
            } else {
                locals.insert(*param_id, variable.clone());
                variable.get()
            };
            call.args.push(StructureField {
                id: *param_id,
                value: Box::new(argument_value(value, is_optional(function, param_id))),
            });
        }
    }

    let result = caller
        .arora_call(call)
        .map_err(BehaviorTreeError::CallError)?;

    for mutated in result.mutated {
        if selected.contains(&mutated.id) {
            continue;
        }
        let variable = locals
            .get(&mutated.id)
            .ok_or(BehaviorTreeError::InternalError {
                message: format!(
                    "mutated parameter {} does not correspond to any local variable",
                    mutated.id
                ),
            })?;
        write_back(variable, *mutated.value, is_optional(function, &mutated.id));
    }

    // If the node has a _ret argument (function does not return a status),
    // write the return value to the bound variable.
    let status = if let Some(var) = locals.get(&_RET_PARAM_ID) {
        write_back(var, result.ret.clone(), function.return_ty.is_option());
        Status::Success
    } else {
        result.ret.try_into().unwrap_or(Status::Success)
    };
    trace_line(status);
    Ok(status)
}

/// Whether `function` declares its parameter `parameter` as an optional.
pub(crate) fn is_optional(function: &Function, parameter: &Uuid) -> bool {
    function
        .parameters
        .get(parameter)
        .is_some_and(|parameter| parameter.ty.is_option())
}

/// The argument a bound value passes to a module function. To an optional
/// parameter, an absent value — a store key that is not set (yet), or a
/// selection over one — passes `None`, so the function decides what an absent
/// value means, and a value passes framed as `Some(value)` (unless it already
/// is an optional). To any other parameter, an absent value passes `()`, which
/// the function refuses unless it takes it.
pub(crate) fn argument_value(value: Option<Value>, optional: bool) -> Value {
    match (value, optional) {
        (Some(value @ Value::Option(_)), _) => value,
        (Some(value), true) => Value::Option(Some(Box::new(value))),
        (Some(value), false) => value,
        (None, true) => Value::Option(None),
        (None, false) => Value::Unit,
    }
}

/// Write a mutated parameter, or a bound return, back to its cell. An optional maps
/// onto the cell's presence: `Some(v)` writes `v` (creating the store key if
/// it did not exist), `None` unsets the cell — and leaves an already-unset one
/// untouched. Any other value is written as is.
pub(crate) fn write_back(variable: &VariableCell, value: Value, optional: bool) {
    match value {
        Value::Option(Some(value)) if optional => variable.set(*value),
        Value::Option(None) if optional => {
            if variable.get().is_some() {
                variable.set_opt(None);
            }
        }
        value => variable.set(value),
    }
}

/// Specialization of Callable that returns a Status.
trait Tickable {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError>;
}

impl Tickable for TickId {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        CallableId {
            id: self.callable_id,
        }
        .tick(caller)
    }
}

impl Tickable for CallableId {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        let value = self.call(caller).map_err(BehaviorTreeError::CallError)?;
        value.try_into().map_err(|_| {
            BehaviorTreeError::ConversionError(ConversionError {
                message: "return value cannot be interpreted as a Status".to_string(),
            })
        })
    }
}

/// A child edge that can check its cache before entering engine dispatch.
struct TickLink {
    id: TickId,
    function: Rc<TickFunction>,
}
impl Tickable for TickLink {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        if let Some(status) = self.function.cached(caller)? {
            return Ok(status);
        }
        self.id.tick(caller)
    }
}

struct TickFunction {
    state: Rc<incremental::State>,
    incremental: Rc<incremental::Incremental>,
    node: Rc<Node>,
    /// The module function the node calls, resolved against the device's
    /// index when the tree was lowered; `None` for a function the index did
    /// not have (or a native node, which needs none).
    function: Option<ModuleFunction>,
    /// The functions the node's call expressions state, resolved against the
    /// same index, with the node's own function: what a call frames its
    /// arguments by, and what a trace names.
    calls: Rc<FunctionIndex>,
    /// The node's parameters in the order its arguments are evaluated: its
    /// function's parameter order, then any others by id. A call in one
    /// argument runs before a call in the next, every tick.
    argument_order: Rc<[Uuid]>,
    locals: Rc<RefCell<HashMap<Uuid, VariableCell>>>,
    node_arg_variables: Rc<crate::id_hash::IdMap<NodeParameterId, VariableCell>>,
    detached: Rc<Detached>,
    /// The detached nodes the node reads, brought up to date before it ticks.
    pulls: Option<Rc<[usize]>>,
    /// Where the node writes, for an `Unpack` or a `Scatter` node.
    outputs: Option<Outputs>,
    children: Vec<TickLink>,
    trace: TraceTick,
}

impl TickFunction {
    fn cached(&self, caller: &mut dyn CallBridge) -> Result<Option<Status>, BehaviorTreeError> {
        if !self.incremental.enabled.get() {
            return Ok(None);
        }
        // Pulls precede the clean check, including when a parent checks a child
        // without dispatch. A changed port can invalidate this very visit.
        if let Some(pulls) = self.pulls.as_deref() {
            let _detached = incremental::Visit::detached();
            self.detached.pull(
                pulls,
                Scope {
                    variables: &self.locals.borrow(),
                    node_parameters_variables: &self.node_arg_variables,
                    calls: &self.calls,
                },
                caller,
            )?;
        }
        if self.state.cacheable && !self.state.dirty.get() && incremental::store_moved() {
            for cell in self.state.subtree_stored.borrow().iter() {
                cell.observe();
            }
        }
        if self.state.cacheable && !self.state.dirty.get() {
            if self.state.pulls {
                self.incremental.visited(&self.state);
            }
            Ok(self
                .state
                .status
                .get()
                .filter(|status| *status != Status::Running))
        } else {
            Ok(None)
        }
    }
}

impl Tickable for TickFunction {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        if let Some(status) = self.cached(caller)? {
            return Ok(status);
        }
        let enabled = self.incremental.enabled.get();
        // A dirty visit reads live slots too. Bring just its own read versions
        // up to date before recording the argument snapshot; a raw write by an
        // earlier sibling must not look like a change during this visit.
        if enabled && incremental::store_moved() {
            for cell in &self.state.reads {
                if matches!(cell, VariableCell::Stored(_)) {
                    cell.observe();
                }
            }
        }
        let versions: Vec<_> = if enabled {
            self.state
                .arguments
                .iter()
                .map(VariableCell::version)
                .collect()
        } else {
            Vec::new()
        };
        self.state.dirty.set(false);
        let _visit = incremental::Visit::enter(&self.state);
        if enabled && !self.state.cacheable {
            // A module composite can write raw store slots before ticking its
            // children, so their cache checks must already observe live reads.
            incremental::store_written();
        }
        let result = tick(
            caller,
            self.function.as_ref(),
            &self.calls,
            &self.argument_order,
            self.locals.clone(),
            self.node_arg_variables.clone(),
            (&self.detached, self.pulls.as_deref()),
            self.outputs.as_ref(),
            &self.children,
            self.node.clone(),
            self.trace,
        );
        if enabled {
            self.state.status.set(result.as_ref().ok().copied());
            if self.state.dirty.get()
                || self
                    .state
                    .arguments
                    .iter()
                    .zip(versions)
                    .any(|(cell, version)| cell.version() != version)
            {
                self.state.mark();
            }
            self.incremental.visited(&self.state);
        }
        result
    }
}

impl Callable for TickFunction {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
        self.tick(caller)
            .map(Into::<Value>::into)
            .map_err(Into::<CallError>::into)
    }
}

// Loading behavior trees.
//====================================================================
pub fn load_behavior_tree_nodes(nodes: Vec<Node>) -> Result<BehaviorTree, BehaviorTreeError> {
    // The bare schema (YAML) path has no variable names to resolve against a
    // store, so every cell stays tree-local.
    load_behavior_tree_nodes_with(nodes, &|_| None, &HashMap::new())
}

/// Like [`load_behavior_tree_nodes`], but binds each `{var}` whose name
/// `resolver` resolves to a data-store slot (the Direct convention — variable
/// name == store key). `names` maps each variable id to the name the resolver
/// expects; ids absent from it (or that the resolver declines) stay tree-local.
/// This is the path the shared-graph lowering
/// ([`build_behavior_tree`](crate::graph::build_behavior_tree)) uses.
///
/// The first node is the root. A call to a module function that the tree does
/// not hold, being neither the root nor a child of a held node, is detached
/// ([`detached`]): it runs when what it outputs is read.
pub fn load_behavior_tree_nodes_with(
    nodes: Vec<Node>,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) -> Result<BehaviorTree, BehaviorTreeError> {
    let mut node_index: HashMap<Uuid, Rc<Node>> = HashMap::new();
    let mut listed: Vec<Rc<Node>> = Vec::new();
    let mut root: Option<Rc<Node>> = None;
    let mut variables = HashMap::new();
    let mut node_parameters_variables = crate::id_hash::IdMap::default();
    let mut arguments = Vec::new();
    let mut outputs = HashMap::new();
    for mut node in nodes {
        node.arguments = node
            .arguments
            .into_iter()
            .map(|(parameter, argument)| (parameter, with_call_site_cells(argument)))
            .collect();
        let shared_node = Rc::new(node);
        if root.is_none() {
            // first node is the root?
            root = Some(shared_node.clone());
        }
        listed.push(shared_node.clone());

        // Index the node and check for duplicates.
        let existing_node = node_index.insert(shared_node.id, shared_node.clone());
        if let Some(existing_node) = existing_node {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: format!("duplicate node {}", existing_node.id),
            });
        }

        for (param_id, arg_expr) in &shared_node.arguments {
            arguments.push((
                NodeParameterId {
                    node: shared_node.id.to_owned(),
                    parameter: param_id.to_owned(),
                },
                arg_expr.clone(),
            ));
        }
    }

    // The nodes the tree holds: the root, and the children of a held node.
    // A call no node holds is detached (see [`detached`]), and its return
    // value is a port whether or not `_ret` is bound.
    let mut attached = HashSet::new();
    let mut held: Vec<Uuid> = root.iter().map(|root| root.id).collect();
    while let Some(id) = held.pop() {
        if attached.insert(id) {
            if let Some(children) = node_index.get(&id).and_then(|node| node.children.as_ref()) {
                held.extend(children.iter().copied());
            }
        }
    }
    let detached_nodes = Detached::select(&listed, &attached);
    let detached_ids: HashSet<Uuid> = detached_nodes.iter().map(|node| node.id).collect();
    let mut bindings = crate::id_hash::IdMap::default();
    let mut fed_inputs = HashSet::new();
    for node in &detached_nodes {
        if !node.arguments.contains_key(&_RET_PARAM_ID) {
            node_parameters_variables.insert(
                NodeParameterId {
                    node: node.id,
                    parameter: _RET_PARAM_ID,
                },
                VariableCell::local(),
            );
        }
    }

    // Setup variables for every parameter. An argument that reads another
    // node's argument shares its cell, so it is set up once that one is: the
    // pending arguments are swept until none is left, and a sweep that sets up
    // none reports what it could not resolve.
    let mut pending = arguments;
    while !pending.is_empty() {
        let before = pending.len();
        let mut unresolved = Vec::new();
        for (node_param, arg_expr) in pending {
            if read_argument(&arg_expr)
                .is_some_and(|source| !node_parameters_variables.contains_key(source))
            {
                unresolved.push((node_param, arg_expr));
                continue;
            }
            let cell = setup_node_parameter_variable(
                &node_param,
                &arg_expr,
                &mut variables,
                &mut node_parameters_variables,
                resolver,
                names,
            )?;
            // A detached node's port bound to a store key keeps a tree-local
            // cell of its own, which `@node` reads; the key is published to
            // after the tree ticks. A return bound to another node's input
            // feeds the input its chain of ports ends at: when that input
            // names a store key, the two share a tree-local cell, every port
            // on the chain follows once the aliases do, and the key is not
            // written. A detached input on the chain may already have a cell
            // of its own, so the end of the chain decides.
            let detached = detached_ids.contains(&node_param.node);
            let fed = read_argument(&arg_expr)
                .filter(|_| detached && node_param.parameter == _RET_PARAM_ID)
                .map(|input| {
                    let mut fed = input.clone();
                    for _ in 0..node_index.len() {
                        let next = node_index
                            .get(&fed.node)
                            .and_then(|node| node.arguments.get(&fed.parameter))
                            .and_then(read_argument);
                        match next {
                            Some(next) => fed = next.clone(),
                            None => break,
                        }
                    }
                    fed
                })
                .filter(|fed| {
                    matches!(
                        node_parameters_variables.get(fed),
                        Some(VariableCell::Stored(_))
                    )
                });
            if let Some(fed) = fed {
                let port = VariableCell::local();
                node_parameters_variables.insert(node_param.clone(), port.clone());
                node_parameters_variables.insert(fed.clone(), port);
                fed_inputs.insert(fed);
            } else if matches!(cell, VariableCell::Stored(_)) && detached {
                node_parameters_variables.insert(node_param.clone(), VariableCell::local());
                bindings.insert(node_param, cell);
            }
        }
        if let Some((node_param, arg_expr)) =
            unresolved.first().filter(|_| unresolved.len() == before)
        {
            setup_node_parameter_variable(
                node_param,
                arg_expr,
                &mut variables,
                &mut node_parameters_variables,
                resolver,
                names,
            )?;
        }
        pending = unresolved;
    }

    // An argument that reads a fed input, at any remove, shares the cell it
    // now has, wherever the sweep set it up: the cell of the last fed input of
    // its chain, which no fed input aliases, so the result does not depend on
    // the order the arguments are visited in.
    if !fed_inputs.is_empty() {
        let sources: HashMap<NodeParameterId, NodeParameterId> = node_index
            .values()
            .flat_map(|node| {
                node.arguments.iter().filter_map(|(parameter, argument)| {
                    read_argument(argument).map(|source| {
                        (
                            NodeParameterId {
                                node: node.id,
                                parameter: *parameter,
                            },
                            source.clone(),
                        )
                    })
                })
            })
            .collect();
        for reader in sources.keys() {
            let mut at = reader;
            let mut fed = None;
            for _ in 0..sources.len() {
                let Some(source) = sources.get(at) else {
                    break;
                };
                if fed_inputs.contains(source) {
                    fed = Some(source);
                }
                at = source;
            }
            if let Some(cell) = fed.and_then(|fed| node_parameters_variables.get(fed).cloned()) {
                node_parameters_variables.insert(reader.clone(), cell);
            }
        }
    }

    // The data nodes, once every argument has its cell.
    for node in node_index.values() {
        if let Some(bound) = bind_data_node(node, names, &node_parameters_variables)? {
            outputs.insert(node.id, bound);
        }
    }
    let detached = Detached::new(detached_nodes, bindings);

    Ok(BehaviorTree {
        root: root.ok_or(BehaviorTreeError::InconsistentTreeError {
            message: "behavior tree has no nodes".to_string(),
        })?,
        node_index,
        variables: Rc::new(RefCell::new(variables)),
        node_arg_variables: Rc::new(node_parameters_variables),
        outputs,
        detached: Rc::new(detached),
    })
}

pub fn load_behavior_tree_yaml(yaml: &str) -> Result<BehaviorTree, BehaviorTreeError> {
    load_behavior_tree_nodes(serde_yaml::from_str(yaml)?)
}

// Other helpers
//=======================================================
fn get_variable<'a>(
    variables: &'a HashMap<Uuid, VariableCell>,
    variable_id: &Uuid,
    node_id: &Uuid,
) -> Result<&'a VariableCell, BehaviorTreeError> {
    variables
        .get(variable_id)
        .ok_or(BehaviorTreeError::VariableNotFound {
            variable: variable_id.to_owned(),
            node: node_id.to_owned(),
        })
}

/// Check a data node's arguments, and bind where an `Unpack` or a `Scatter`
/// node writes: `None` for any other node.
///
/// `Equal` needs both its arguments, `Unpack` its values and `Scatter` its
/// values and keys, linked. Each of `Unpack`'s other arguments is an output
/// port bound to a variable, whose cell the generic lowering already set up;
/// two ports may not bind one variable, by id or by name. `Scatter`'s
/// `within`, if linked, is a literal key prefix, the tree's whole namespace
/// when empty (see [`scatter::Scatter::bind`]).
pub(crate) fn bind_data_node(
    node: &Node,
    names: &HashMap<Uuid, String>,
    node_parameters_variables: &crate::id_hash::IdMap<NodeParameterId, VariableCell>,
) -> Result<Option<Outputs>, BehaviorTreeError> {
    let refuse = |what: String| {
        Err(BehaviorTreeError::InconsistentTreeError {
            message: format!("node {}: {what}", node.id),
        })
    };
    let required: &[Uuid] = match node.function {
        EQUAL_FUNCTION_ID => &[EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID],
        UNPACK_FUNCTION_ID => &[UNPACK_VALUES_PARAM_ID],
        SCATTER_FUNCTION_ID => &[SCATTER_VALUES_PARAM_ID, SCATTER_KEYS_PARAM_ID],
        _ => return Ok(None),
    };
    if let Some(missing) = required.iter().find(|id| !node.arguments.contains_key(id)) {
        return refuse(format!("argument {missing} is not linked"));
    }
    if node.function == SCATTER_FUNCTION_ID {
        return Ok(Some(Outputs::Keys(Rc::new(scatter::Scatter::bind(node)?))));
    }
    if node.function != UNPACK_FUNCTION_ID {
        return Ok(None);
    }
    let mut bound = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (param, expression) in &node.arguments {
        if *param == UNPACK_VALUES_PARAM_ID {
            continue;
        }
        let Some(index) = crate::nodes::unpack_out_index(*param) else {
            return refuse(format!("Unpack has no port {param}"));
        };
        let Expression::VariableId(variable) = expression else {
            return refuse(format!(
                "Unpack's out_{index} is written, so it binds a variable, not {expression:?}"
            ));
        };
        let target = names
            .get(variable)
            .map_or_else(|| variable.to_string(), |name| format!("'{name}'"));
        if !seen.insert(target.clone()) {
            return refuse(format!("two of Unpack's ports write the variable {target}"));
        }
        bound.push(index);
    }
    let count = bound.iter().max().map_or(0, |last| last + 1);
    let mut outputs = vec![None; count];
    for index in bound {
        let port = NodeParameterId {
            node: node.id,
            parameter: UNPACK_OUT_PARAM_IDS[index],
        };
        outputs[index] =
            Some(get_node_parameter_variable(&port, node_parameters_variables)?.clone());
    }
    Ok(Some(Outputs::Ports(outputs.into())))
}

/// The other node's argument `expression` reads, through any selection.
fn read_argument(expression: &Expression) -> Option<&NodeParameterId> {
    match expression {
        Expression::NodeArgument(source) => Some(source),
        Expression::Select { source, .. } => read_argument(source),
        _ => None,
    }
}

fn get_node_parameter_variable<'a>(
    node_parameter: &NodeParameterId,
    node_parameters_variables: &'a crate::id_hash::IdMap<NodeParameterId, VariableCell>,
) -> Result<&'a VariableCell, BehaviorTreeError> {
    node_parameters_variables
        .get(node_parameter)
        .ok_or(BehaviorTreeError::InconsistentTreeError {
            message: format!("node parameter {} was not found", node_parameter),
        })
}

/// Sets up a variable for the given node parameter.
/// A variable will always be added to the `node_parameters_variables`.
/// If the parameter refers to a variable that does not exist yet,
/// it will be created with the default value `Value::Unit`.
/// If a parameter has to be computed with a function call,
/// the variable will hold the default value `Value::Unit`.
pub(crate) fn setup_node_parameter_variable(
    node_parameter: &NodeParameterId,
    argument_expression: &Expression,
    variables: &mut HashMap<Uuid, VariableCell>,
    node_parameters_variables: &mut crate::id_hash::IdMap<NodeParameterId, VariableCell>,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) -> Result<VariableCell, BehaviorTreeError> {
    let variable = match argument_expression {
        Expression::Value(value) => VariableCell::local_with(value.to_owned()),
        Expression::Uuid(uuid) => {
            let value = Value::ArrayU8(uuid.as_bytes().to_vec());
            VariableCell::local_with(value)
        }
        Expression::Variable(variable) => variable.clone(),
        Expression::VariableId(variable_id) => {
            variable_cell(*variable_id, variables, resolver, names)
        }
        Expression::NodeArgument(other_node_parameter) => node_parameters_variables
            .get(other_node_parameter)
            .ok_or(BehaviorTreeError::InconsistentTreeError {
                message: format!(
                    "node argument {} used by node argument {} was not found",
                    other_node_parameter,
                    node_parameter.to_owned()
                ),
            })?
            .to_owned(),
        // A selection shares its source's cell; the `Key` path is applied on
        // read (see `compute_expression`), not at bind time.
        Expression::Select { source, .. } => setup_node_parameter_variable(
            node_parameter,
            source,
            variables,
            node_parameters_variables,
            resolver,
            names,
        )?,
        Expression::Call(_) => {
            // The variables the call reads, at any depth, get their cells now:
            // a call reads them when the node ticks.
            bind_variables(argument_expression, variables, resolver, names);
            VariableCell::local()
        }
    };
    node_parameters_variables.insert(node_parameter.to_owned(), variable.to_owned());
    Ok(variable)
}

/// Give every variable `expression` reads, at any depth, its cell.
pub(crate) fn bind_variables(
    expression: &Expression,
    variables: &mut HashMap<Uuid, VariableCell>,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) {
    match expression {
        Expression::VariableId(id) => {
            variable_cell(*id, variables, resolver, names);
        }
        Expression::Select { source, .. } => bind_variables(source, variables, resolver, names),
        Expression::Call(call) => {
            let parts = [&call.module, &call.function].into_iter().chain(
                call.arguments
                    .iter()
                    .flat_map(|(parameter, argument)| [parameter, argument]),
            );
            for part in parts {
                bind_variables(part, variables, resolver, names);
            }
        }
        _ => {}
    }
}

/// The cell of the variable `variable_id`, created at its first reference:
/// bound to the host data store if the resolver knows its name (the Direct
/// convention — variable name == store key), otherwise tree-local. A
/// tree-local name never reaches the resolver. The cell is keyed by its own
/// id, so every reference to the same `{var}` shares it (see the
/// `shared_variable_id_resolves_to_one_cell` regression test).
pub(crate) fn variable_cell(
    variable_id: Uuid,
    variables: &mut HashMap<Uuid, VariableCell>,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) -> VariableCell {
    variables
        .entry(variable_id)
        .or_insert_with(|| {
            names
                .get(&variable_id)
                .filter(|name| !is_tree_local(name))
                .and_then(|name| resolver(name))
                .map(VariableCell::stored)
                .unwrap_or_else(VariableCell::local)
        })
        .clone()
}

/// What an expression is evaluated against: the tree's variables, the cells
/// of the nodes' arguments, and the signatures of the functions its calls
/// state.
#[derive(Clone, Copy)]
pub(crate) struct Scope<'a> {
    pub(crate) variables: &'a HashMap<Uuid, VariableCell>,
    pub(crate) node_parameters_variables: &'a crate::id_hash::IdMap<NodeParameterId, VariableCell>,
    pub(crate) calls: &'a FunctionIndex,
}

pub(crate) fn compute_expression(
    scope: Scope,
    expression: &Expression,
    caller: &mut dyn CallBridge,
    node_parameter: &NodeParameterId,
) -> Result<Value, BehaviorTreeError> {
    let value = match expression {
        Expression::Value(value) => value.to_owned(),
        Expression::Uuid(uuid) => Value::ArrayU8(uuid.as_bytes().to_vec()),
        Expression::Variable(variable) => variable.get_or_unit(),
        Expression::VariableId(variable_id) => {
            let variable = get_variable(scope.variables, variable_id, &node_parameter.node)?;
            variable.get_or_unit()
        }
        Expression::Call(call) => call_expression(scope, call, caller, node_parameter)?,
        Expression::NodeArgument(other_node_parameter) => {
            let variable =
                get_node_parameter_variable(other_node_parameter, scope.node_parameters_variables)?;
            variable.get_or_unit()
        }
        Expression::Select { source, path } => {
            let value = compute_expression(scope, source, caller, node_parameter)?;
            select(path, &value)?
        }
    };
    Ok(value)
}

/// Read `path` out of `value`. A selection over an absent value reads as
/// absent (`Unit`), as the absent value itself does; a path the present value
/// does not have is an error.
fn select(path: &arora_types::data::Key, value: &Value) -> Result<Value, BehaviorTreeError> {
    if *value == Value::Unit {
        return Ok(Value::Unit);
    }
    path.select(value)
        .map_err(|message| BehaviorTreeError::InconsistentTreeError { message })
}

fn compute_uuid(
    scope: Scope,
    expression: &Expression,
    caller: &mut dyn CallBridge,
    node_parameter: &NodeParameterId,
) -> Result<Uuid, BehaviorTreeError> {
    match expression {
        Expression::Uuid(uuid) => Ok(uuid.to_owned()),
        Expression::VariableId(variable_id) => {
            let variable = get_variable(scope.variables, variable_id, &node_parameter.node)?;
            try_into_uuid(&variable.get_or_unit(), &Some(variable_id))
        }
        expression => try_into_uuid(
            &compute_expression(scope, expression, caller, node_parameter)?,
            &None,
        ),
    }
}

fn try_into_uuid(value: &Value, variable_id: &Option<&Uuid>) -> Result<Uuid, BehaviorTreeError> {
    if let Value::ArrayU8(uuid_bytes) = value {
        let uuid = Uuid::from_slice(uuid_bytes.as_slice()).map_err(|e| {
            BehaviorTreeError::ConversionError(ConversionError {
                message: format!(
                    "bytes of variable {:?} are not an uuid ({})",
                    variable_id, e
                ),
            })
        })?;
        Ok(uuid)
    } else {
        Err(BehaviorTreeError::ConversionError(ConversionError {
            message: format!("variable {:?} is not an uuid", variable_id),
        }))
    }
}

/// The cell a mutated parameter of a call is written back to: the variable,
/// anonymous cell or node argument its argument reads. A literal argument is
/// an anonymous cell once the tree is built ([`with_call_site_cells`]). A
/// selection or a computed value has no cell of its own.
pub(crate) fn written_cell(scope: Scope, argument: &Expression) -> Option<VariableCell> {
    match argument {
        Expression::Variable(cell) => Some(cell.clone()),
        Expression::VariableId(id) => scope.variables.get(id).cloned(),
        Expression::NodeArgument(at) => scope.node_parameters_variables.get(at).cloned(),
        _ => None,
    }
}

/// Evaluate a call expression: its module, function and arguments first,
/// innermost call first, then the call itself.
///
/// When the function is one the tree was lowered with ([`Scope::calls`]), an
/// argument is framed as the parameter's type asks: an absent value passes
/// `None` to an optional parameter. A parameter the function reports mutated
/// is written back to the cell its argument reads, which keeps a stateful
/// function's state from one tick to the next.
fn call_expression(
    scope: Scope,
    call: &CallExpression,
    caller: &mut dyn CallBridge,
    node_parameter: &NodeParameterId,
) -> Result<Value, BehaviorTreeError> {
    let module_id = compute_uuid(scope, &call.module, caller, node_parameter)?;
    let function_id = compute_uuid(scope, &call.function, caller, node_parameter)?;
    if is_interpreter_function_id(function_id) {
        // A tree runs inside the interpreter's tick: it cannot call the
        // interpreter back.
        return Err(BehaviorTreeError::InconsistentTreeError {
            message: format!(
                "{node_parameter}: {} calls the interpreter module's function {function_id}, \
                 which a tree does not call",
                Expression::Call(Box::new(call.clone())).display(Names::functions(scope.calls))
            ),
        });
    }
    let function = scope.calls.get(&function_id).map(|f| &f.function);
    let mut args = Vec::with_capacity(call.arguments.len());
    let mut written = Vec::new();
    let mut unwritable = Vec::new();
    for (arg_id_expression, value_expression) in &call.arguments {
        let arg_id = compute_uuid(scope, arg_id_expression, caller, node_parameter)?;
        let value = compute_expression(scope, value_expression, caller, node_parameter)?;
        let optional = function.is_some_and(|function| is_optional(function, &arg_id));
        match written_cell(scope, value_expression) {
            Some(cell) => written.push((arg_id, cell, optional)),
            None => unwritable.push(arg_id),
        }
        args.push(StructureField {
            id: arg_id,
            value: Box::new(argument_value(
                Some(value).filter(|value| *value != Value::Unit),
                optional,
            )),
        });
    }
    let result = caller
        .arora_call(Call {
            module_id: Some(module_id),
            id: function_id,
            args,
        })
        .map_err(BehaviorTreeError::CallError)?;
    // Lowering refuses a mutable parameter fed by a selection or a call when
    // the call states its parameter; one it computes is checked here, when
    // the function writes it back.
    for mutated in result.mutated {
        if let Some((_, cell, optional)) = written.iter().find(|(id, ..)| *id == mutated.id) {
            write_back(cell, *mutated.value, *optional);
        } else if unwritable.contains(&mutated.id) {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: format!(
                    "{node_parameter}: {} writes back parameter {}, whose argument, a selection \
                     or a call, has no cell to hold it",
                    Expression::Call(Box::new(call.clone())).display(Names::functions(scope.calls)),
                    mutated.id
                ),
            });
        }
    }
    Ok(result.ret)
}

/// Whether `function` is one of the interpreter module's own functions
/// (`load`, `edit`, `spawn`, `spawn_graph`, `halt`). They act on the
/// interpreter, which a tree cannot call while it is being ticked, so a tree
/// names none of them.
pub(crate) fn is_interpreter_function(function: &ModuleFunction) -> bool {
    use arora_behavior::interpreter_module::{EDIT, HALT, ID, LOAD, SPAWN, SPAWN_GRAPH};
    function.module_id == ID
        && [LOAD, EDIT, SPAWN, SPAWN_GRAPH, HALT].contains(&function.function_id)
}

/// Whether `function` is the id of one of the interpreter module's own
/// functions; see [`is_interpreter_function`].
pub(crate) fn is_interpreter_function_id(function: Uuid) -> bool {
    use arora_behavior::interpreter_module::{EDIT, HALT, LOAD, SPAWN, SPAWN_GRAPH};
    [LOAD, EDIT, SPAWN, SPAWN_GRAPH, HALT].contains(&function)
}

// Call expressions.
//=======================================================
/// Every call expression in `expression`, the nested ones included, outermost
/// first.
fn calls_in<'e>(expression: &'e Expression, found: &mut Vec<&'e CallExpression>) {
    match expression {
        Expression::Call(call) => {
            found.push(call);
            calls_in(&call.module, found);
            calls_in(&call.function, found);
            for (parameter, argument) in &call.arguments {
                calls_in(parameter, found);
                calls_in(argument, found);
            }
        }
        Expression::Select { source, .. } => calls_in(source, found),
        _ => {}
    }
}

/// The signatures of the functions `node`'s call expressions state, and of
/// the node's own function, from `functions`: what a call frames its arguments
/// by and a trace names. A call whose function is computed, or not in
/// `functions`, is framed as it is given and fails when it ticks if the
/// function does not exist, as a module leaf does.
///
/// Refuses, naming the call:
/// - a call to the interpreter module's functions, which a tree does not call;
/// - an argument for a parameter the function does not have;
/// - a mutable parameter fed by a selection or a call: the function writes it
///   back, and neither has a cell to hold what it writes. A literal
///   holds it at the call site, a variable or a node argument in its cell.
fn call_signatures(
    node: &Node,
    functions: &FunctionIndex,
) -> Result<FunctionIndex, BehaviorTreeError> {
    let mut signatures = expression_call_signatures(
        || format!("node {}", node.id),
        node.arguments.values(),
        functions,
    )?;
    if let Some(function) = functions.get(&node.function) {
        signatures.insert(node.function, function.clone());
    }
    Ok(signatures)
}

/// The signatures of the functions the calls in `arguments` state, checked as
/// [`call_signatures`] says; `owner` names what holds them in a refusal.
pub(crate) fn expression_call_signatures<'e>(
    owner: impl Fn() -> String,
    arguments: impl IntoIterator<Item = &'e Expression>,
    functions: &FunctionIndex,
) -> Result<FunctionIndex, BehaviorTreeError> {
    let mut signatures = FunctionIndex::new();
    let mut calls = Vec::new();
    for argument in arguments {
        calls_in(argument, &mut calls);
    }
    if calls.is_empty() {
        return Ok(signatures);
    }
    let names = Names::functions(functions);
    let refuse = |call: &CallExpression, why: String| {
        Err(BehaviorTreeError::InconsistentTreeError {
            message: format!(
                "{}: {} {why}",
                owner(),
                Expression::Call(Box::new(call.clone())).display(names)
            ),
        })
    };
    for call in calls {
        let Some(function_id) = stated_id(&call.function) else {
            continue;
        };
        if is_interpreter_function_id(function_id) {
            return refuse(
                call,
                "calls the interpreter module, which a tree does not call".to_string(),
            );
        }
        let Some(signature) = functions.get(&function_id) else {
            continue;
        };
        for (parameter, argument) in &call.arguments {
            let Some(parameter_id) = stated_id(parameter) else {
                continue;
            };
            let Some(declared) = signature.function.parameters.get(&parameter_id) else {
                return refuse(
                    call,
                    format!(
                        "names the parameter {parameter_id}, which {} does not have",
                        signature.function_name
                    ),
                );
            };
            if declared.mutable
                && matches!(argument, Expression::Select { .. } | Expression::Call(_))
            {
                return refuse(
                    call,
                    format!(
                        "feeds the mutable parameter `{}` with {}: what the function writes \
                         back needs a literal, a variable or a node argument to hold it",
                        declared.name,
                        argument.display(names)
                    ),
                );
            }
        }
        signatures.insert(function_id, signature.clone());
    }
    Ok(signatures)
}

/// `expression` with each literal argument of its calls held in a cell of its
/// own, an anonymous [`Expression::Variable`]: the call site's state. A
/// mutable parameter fed a literal starts from that literal and keeps what
/// the function writes back from one tick to the next, as a module leaf's
/// literal argument does. Two call sites of one function keep two states.
/// The calls that compute a module, function or parameter id get their cells
/// too; a literal id stays as it is.
pub(crate) fn with_call_site_cells(expression: Expression) -> Expression {
    let identifier = |expression: Expression| match expression {
        Expression::Call(_) | Expression::Select { .. } => with_call_site_cells(expression),
        expression => expression,
    };
    match expression {
        Expression::Call(call) => {
            let CallExpression {
                module,
                function,
                arguments,
            } = *call;
            Expression::Call(Box::new(CallExpression {
                module: identifier(module),
                function: identifier(function),
                arguments: arguments
                    .into_iter()
                    .map(|(parameter, argument)| {
                        let parameter = identifier(parameter);
                        let argument = match argument {
                            Expression::Value(value) => {
                                Expression::Variable(VariableCell::local_with(value))
                            }
                            Expression::Uuid(id) => Expression::Variable(VariableCell::local_with(
                                Value::ArrayU8(id.as_bytes().to_vec()),
                            )),
                            argument => with_call_site_cells(argument),
                        };
                        (parameter, argument)
                    })
                    .collect(),
            }))
        }
        Expression::Select { source, path } => Expression::Select {
            source: Box::new(with_call_site_cells(*source)),
            path,
        },
        expression => expression,
    }
}

/// Move each call-site cell `held` holds into the matching cell of
/// `expression`: the same node of a tree built again from an unchanged graph,
/// so the two have the same shape.
fn take_call_site_cells(expression: &Expression, held: &Expression) {
    match (expression, held) {
        (Expression::Call(call), Expression::Call(held)) => {
            take_call_site_cells(&call.module, &held.module);
            take_call_site_cells(&call.function, &held.function);
            for ((parameter, argument), (held_parameter, held)) in
                call.arguments.iter().zip(&held.arguments)
            {
                take_call_site_cells(parameter, held_parameter);
                take_call_site_cells(argument, held);
            }
        }
        (Expression::Select { source, .. }, Expression::Select { source: held, .. }) => {
            take_call_site_cells(source, held)
        }
        (Expression::Variable(cell), Expression::Variable(held)) => take_local(cell, held),
        _ => {}
    }
}
