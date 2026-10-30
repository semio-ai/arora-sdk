//! Detached nodes: the nodes of a tree that no node holds.
//!
//! A node is **attached** when the tree holds it: it is the root, or a child
//! of an attached node. A call to a module function that no node holds, and
//! that holds no children, is **detached**. Another node no node holds, such
//! as a control node an edit adds before the edit that places it, waits until
//! a node holds it.
//! Both are described alike, as a [`Node`]: a function, and one argument per
//! parameter. Their outputs are bound alike too: the return value through the
//! `_ret` argument, a mutable parameter through the cell its argument names.
//! Attachment is the only difference, and it decides when the node runs. An
//! attached node runs when its parent ticks it. A detached node runs whenever
//! its outputs are about to be read: when it computes does not matter, only
//! that what it outputs is up to date when it is used.
//!
//! ```text
//! raw: mul(a: {sensor/distance}, b: 100f64)
//! cm: clamp(x: @raw._ret, lo: 0f64)
//! level: {face/level} = add(a: @cm._ret, b: 1f64)
//! ```
//!
//! # What reads a detached node
//!
//! Nothing marks a read. A node reads a detached node when one of its
//! arguments reads a tree-local cell the detached node writes: its return
//! value, as `@node._ret` ([`Expression::NodeArgument`] on `_ret`) or through the
//! variable `_ret` is bound to, or one of its mutable parameters, as
//! `@node.parameter` or through its variable. Lowering finds the writer of
//! every such cell, and gives each node the detached nodes it reads. A
//! detached node's return value is a port even when `_ret` is not bound. An
//! output bound to another node's input, as `_ret: @reader.x`, writes the cell
//! that input reads, so the reader reads the detached node. When the input
//! names a store key, the two share a tree-local cell, and the key is not
//! written.
//!
//! A store key is not such a cell. A port bound to a store key keeps a
//! tree-local cell of its own, which `@node._ret` reads, and publishes to the key
//! after the tree ticks. The key itself is read as the store holds it, so a
//! value fed back through a key, or passed from one graph to another through a
//! key, reaches its reader at the next tick. Feedback goes only through a key:
//! a detached node reads its own state, but no other output of its own, and a
//! call in its arguments writes back no key.
//!
//! # Evaluation
//!
//! - **Before its reader.** A node that ticks first brings up to date the
//!   detached nodes it reads, and a detached node first brings up to date
//!   those it reads. A detached node that nothing reads is not evaluated,
//!   unless it writes a store key ([`DetachedPlan`]).
//! - **Once per tick.** The first read of a detached node in a tick brings it
//!   up to date. Every later read in the same tick reuses that result. What
//!   the tree writes after that first read reaches the node at the next tick.
//! - **Only when its inputs changed.** A detached node calls its function only
//!   when its argument values differ from those of its last call. A settled
//!   graph makes no calls. A changed input calls the nodes downstream of it,
//!   and no others. A value that changes at every tick, such as the clock,
//!   calls the nodes that read it at every tick.
//! - **State lives in a mutable parameter.** A literal argument to it is the
//!   node's own cell, which starts from the literal and holds what the
//!   function writes back. A changed state is a changed input, so the node is
//!   called until its state settles.
//!
//! # Sinks
//!
//! A detached node with an output bound to a **store key** is a sink. Its
//! readers are outside the tree, so it is brought up to date at **every
//! tick**, after the tree has ticked, even when the tree failed; a sink that
//! fails stops no other, and publishes nothing that tick. Once every sink is
//! up to date, each publishes its keys, in the order the tree lists them, even
//! when its result comes from its cache: a sink never reads another's key of
//! the same tick, and of two sinks of one key, the later listed wins, as the
//! later writer does in a node graph. A tree built from the shared `Graph`,
//! which keeps no order among its nodes, lists them by id after the root.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use arora_behavior::{Activity, FunctionIndex};
use arora_types::call::{CallBridge, CallError};
use arora_types::value::Value;
use uuid::Uuid;

use crate::display::{stated_id, Names};
use crate::error::BehaviorTreeError;
use crate::nodes::{
    REGEX_MATCH_FIRST_MATCH_PARAM_ID, REGEX_MATCH_FUNCTION_ID, RUN_STATUS_FUNCTION_ID,
    RUN_STATUS_LATCH_PARAM_ID, RUN_STATUS_OUT_PARAM_ID, SEQ_STAR_CURRENT_INDEX_PARAM_ID,
    SEQ_STAR_FUNCTION_ID, SET_STR_FUNCTION_ID, SET_STR_VARIABLE_PARAM_ID, UNPACK_FUNCTION_ID,
    UNPACK_OUT_PARAM_IDS, UNSET_STR_FUNCTION_ID, UNSET_STR_VARIABLE_PARAM_ID,
};
use crate::schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID};
use crate::variable::VariableCell;
use crate::{
    argument_value, compute_expression, expression_call_signatures, is_interpreter_function_id,
    is_native, is_optional, write_back, Scope,
};

/// How a tree's detached nodes are evaluated, from what reads them. Each list
/// holds node ids in evaluation order, inputs before their readers, and each
/// detached node is in exactly one of them. Known once the tree is lowered.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DetachedPlan {
    /// The sinks, the nodes a host observes, and the nodes they read:
    /// brought up to date at every tick.
    pub every_tick: Vec<Uuid>,
    /// The nodes the tree reads and the nodes they read, apart from the
    /// above: brought up to date at a tick only when a node that reads them
    /// ticks.
    pub on_demand: Vec<Uuid>,
    /// The nodes nothing reads and that write no store key: never evaluated.
    pub unused: Vec<Uuid>,
}

/// A tree's detached nodes, ready to evaluate once lowered.
pub(crate) struct Detached {
    /// The detached nodes, in the order the tree lists them.
    nodes: Vec<Rc<Node>>,
    /// Each node's position in `nodes`, by id.
    ids: crate::id_hash::IdMap<Uuid, usize>,
    /// The store keys the ports of detached nodes publish to, by port; the
    /// port itself is the tree-local cell the node's argument has.
    bindings: crate::id_hash::IdMap<NodeParameterId, VariableCell>,
    /// What lowering found: who reads whom, and what each node writes.
    lowered: RefCell<Lowered>,
    caches: RefCell<Vec<Cache>>,
    /// The current tick: a node whose cache is of this tick is up to date.
    tick: Cell<u64>,
    /// The nodes a host observes: brought up to date at every tick, as a
    /// sink is, whatever reads them.
    observed: RefCell<HashSet<Uuid>>,
    /// The stack a node is brought up to date with, kept between ticks.
    stack: RefCell<Vec<(usize, usize)>>,
    /// Where each node records what it did, while a trace runs.
    record: Rc<crate::trace::TickRecord>,
}

#[derive(Default)]
struct Lowered {
    /// The functions the detached nodes call, and those their nested calls
    /// state.
    signatures: FunctionIndex,
    /// By node: the detached nodes it reads, by position.
    reads: Vec<Vec<usize>>,
    /// By node: its arguments in signature order, `_ret` aside, each with
    /// where its value comes from, resolved once.
    arguments: Vec<Vec<Argument>>,
    /// By node: the module its function belongs to, when the function index
    /// has the function.
    modules: Vec<Option<Uuid>>,
    /// By node: whether something besides its own call writes its output
    /// cells (a nested call of its arguments, or the store a mutable port is
    /// seeded from), so an evaluation that does not call writes its cached
    /// outputs back.
    rewrites: Vec<bool>,
    /// By node: its outputs, each a port, the tree-local cell it writes, and
    /// the store key it publishes to after the tree ticks.
    outputs: Vec<Vec<Output>>,
    /// By node: whether every argument reads a cell, so the arguments of its
    /// last call compare with them in place.
    cells_only: Vec<bool>,
    /// The sinks, by position, in the order the tree lists them.
    sinks: Vec<usize>,
    /// The observed nodes that are no sinks, by position, in evaluation
    /// order.
    observed: Vec<usize>,
    /// By attached node: the detached nodes it reads, by position.
    pulls: HashMap<Uuid, Rc<[usize]>>,
    plan: DetachedPlan,
}

/// An argument of a detached node, resolved when the tree is lowered.
struct Argument {
    parameter: Uuid,
    /// Whether the parameter is optional: an absent value passes `None`.
    optional: bool,
    source: Source,
}

/// Where an argument's value comes from at each tick.
enum Source {
    /// The cell it reads: a literal's own, a variable's, another node's port,
    /// or the store key bound to it.
    Cell(Option<VariableCell>),
    /// A call or a selection, computed from the expression.
    Computed(Expression),
}

#[derive(Default, Clone)]
struct Cache {
    /// The tick the node was last brought up to date at.
    tick: u64,
    /// The arguments of its last call.
    arguments: Option<Vec<Value>>,
    /// Its outputs after its last call, each a port and its value.
    outputs: Vec<(Uuid, Value)>,
    /// How many times it called its function.
    calls: u64,
    /// Why its evaluation failed at `tick`, if it did: a later read in the
    /// same tick fails alike without calling again.
    failed: Option<String>,
}

fn inconsistent(message: String) -> BehaviorTreeError {
    BehaviorTreeError::InconsistentTreeError { message }
}

/// The identity of a cell: two clones of one cell are the same cell.
fn cell_id(cell: &VariableCell) -> usize {
    match cell {
        VariableCell::Local(cell) => Rc::as_ptr(cell) as *const () as usize,
        VariableCell::Stored(slot) => Rc::as_ptr(slot) as *const () as usize,
    }
}

/// The cells `expression` reads, at any depth.
fn cells_read(
    expression: &Expression,
    variables: &HashMap<Uuid, VariableCell>,
    node_parameters_variables: &crate::id_hash::IdMap<NodeParameterId, VariableCell>,
    found: &mut Vec<VariableCell>,
) {
    match expression {
        Expression::Variable(cell) => found.push(cell.clone()),
        Expression::VariableId(id) => found.extend(variables.get(id).cloned()),
        Expression::NodeArgument(source) => {
            found.extend(node_parameters_variables.get(source).cloned())
        }
        Expression::Select { source, .. } => {
            cells_read(source, variables, node_parameters_variables, found)
        }
        Expression::Call(call) => {
            let parts = [&call.module, &call.function].into_iter().chain(
                call.arguments
                    .iter()
                    .flat_map(|(parameter, argument)| [parameter, argument]),
            );
            for part in parts {
                cells_read(part, variables, node_parameters_variables, found);
            }
        }
        Expression::Value(_) | Expression::Uuid(_) => {}
    }
}

/// Whether a node calling `function`, with `children`, is detached when no
/// node holds it: a call to a module function, holding no children.
pub(crate) fn is_detachable(function: Uuid, children: Option<&[Uuid]>) -> bool {
    !is_native(function) && children.is_none_or(|children| children.is_empty())
}

impl Detached {
    /// The detached nodes of `nodes`, in their order: the calls to a module
    /// function, with no children, that `attached` does not hold.
    pub(crate) fn select(nodes: &[Rc<Node>], attached: &HashSet<Uuid>) -> Vec<Rc<Node>> {
        nodes
            .iter()
            .filter(|node| {
                !attached.contains(&node.id)
                    && is_detachable(node.function, node.children.as_deref())
            })
            .cloned()
            .collect()
    }

    /// The detached `nodes`, whose ports publish to the store keys
    /// `bindings` name.
    pub(crate) fn new(
        nodes: Vec<Rc<Node>>,
        bindings: crate::id_hash::IdMap<NodeParameterId, VariableCell>,
        record: Rc<crate::trace::TickRecord>,
    ) -> Self {
        let caches = RefCell::new(vec![Cache::default(); nodes.len()]);
        Self {
            ids: nodes
                .iter()
                .enumerate()
                .map(|(at, node)| (node.id, at))
                .collect(),
            nodes,
            bindings,
            lowered: RefCell::default(),
            caches,
            tick: Cell::new(1),
            observed: RefCell::default(),
            stack: RefCell::default(),
            record,
        }
    }

    /// No detached node, recording to `record`.
    pub(crate) fn empty(record: Rc<crate::trace::TickRecord>) -> Self {
        Self {
            nodes: Vec::new(),
            ids: crate::id_hash::IdMap::default(),
            bindings: crate::id_hash::IdMap::default(),
            lowered: RefCell::default(),
            caches: RefCell::default(),
            tick: Cell::new(1),
            observed: RefCell::default(),
            stack: RefCell::default(),
            record,
        }
    }

    /// Observe the detached nodes among `ids` from the next lowering on:
    /// bring them up to date at every tick, after the sinks.
    pub(crate) fn observe(&self, ids: impl IntoIterator<Item = Uuid>) {
        self.observed
            .borrow_mut()
            .extend(ids.into_iter().filter(|id| self.ids.contains_key(id)));
    }

    /// The output `port` of the detached node `id` after its last call.
    pub(crate) fn output(&self, id: Uuid, port: Uuid) -> Option<Value> {
        let at = *self.ids.get(&id)?;
        let caches = self.caches.borrow();
        caches[at].arguments.as_ref()?;
        caches[at]
            .outputs
            .iter()
            .find(|(output, _)| *output == port)
            .map(|(_, value)| value.clone())
    }

    /// The outputs of the detached node `id` after its last call, by port:
    /// its return value under `_ret`, each mutable parameter under its id.
    /// `None` for a node that is not detached or has not been called.
    pub(crate) fn outputs(&self, id: Uuid) -> Option<HashMap<Uuid, Value>> {
        let at = *self.ids.get(&id)?;
        let caches = self.caches.borrow();
        caches[at]
            .arguments
            .is_some()
            .then(|| caches[at].outputs.iter().cloned().collect())
    }

    /// The detached nodes, in the order the tree lists them.
    pub(crate) fn nodes(&self) -> &[Rc<Node>] {
        &self.nodes
    }

    pub(crate) fn plan(&self) -> DetachedPlan {
        self.lowered.borrow().plan.clone()
    }

    /// How many times the detached node `id` called its function.
    pub(crate) fn calls(&self, id: Uuid) -> Option<u64> {
        let at = *self.ids.get(&id)?;
        Some(self.caches.borrow()[at].calls)
    }

    /// The detached nodes `node` reads, for its tick to bring up to date.
    pub(crate) fn pulls(&self, node: &Uuid) -> Option<Rc<[usize]>> {
        self.lowered.borrow().pulls.get(node).cloned()
    }

    /// Find who reads whom, against `functions`, which tell each node's
    /// outputs: its return value and its mutable parameters.
    ///
    /// Refuses, naming the node: a call to the interpreter module; an argument
    /// for a parameter the function does not have; a mutable parameter fed by
    /// a selection or a call, neither of which has a cell to hold what the
    /// function writes back; a tree-local cell two nodes write; a call in a
    /// detached node's arguments that writes back to a store key, which a
    /// detached node writes only through its outputs; an output bound to an
    /// input a call or a selection computes; a mutable parameter bound to
    /// another node's input that names a store key; a read of a mutable
    /// parameter's port that is no output; a detached node reading its own
    /// output other than as its state; and detached nodes that read each other
    /// in a cycle. A function `functions` does not have fails the node when it
    /// is evaluated, as a module leaf fails.
    ///
    /// A call whose function or parameter is computed may write back any cell
    /// its arguments name, so each such cell counts as written.
    pub(crate) fn lower(
        &self,
        functions: &FunctionIndex,
        attached: &[Rc<Node>],
        variables: &HashMap<Uuid, VariableCell>,
        node_parameters_variables: &crate::id_hash::IdMap<NodeParameterId, VariableCell>,
    ) -> Result<(), BehaviorTreeError> {
        let names = Names::functions(functions);
        let mut signatures = FunctionIndex::new();
        let mut arguments = Vec::with_capacity(self.nodes.len());
        let mut modules = Vec::with_capacity(self.nodes.len());
        let mut rewrites = Vec::with_capacity(self.nodes.len());
        let mut positions: crate::id_hash::IdMap<Uuid, crate::id_hash::IdMap<Uuid, usize>> =
            crate::id_hash::IdMap::default();
        let mut outputs = Vec::with_capacity(self.nodes.len());
        // The cells each detached node keeps its state in, which it reads and
        // writes back: its mutable parameters' and its calls'.
        let mut states: Vec<HashSet<usize>> = Vec::with_capacity(self.nodes.len());
        // The writer of each tree-local cell a node outputs to, attached or
        // detached: `Some(position)` for a detached node.
        let mut writers: HashMap<usize, (Option<usize>, Uuid)> = HashMap::new();
        let mut claim = |cell: &VariableCell,
                         writer: Option<usize>,
                         node: Uuid|
         -> Result<(), BehaviorTreeError> {
            if matches!(cell, VariableCell::Stored(_)) {
                return Ok(());
            }
            // The tree's own nodes may write one cell in turn; a cell a
            // detached node writes has that one writer.
            match writers.insert(cell_id(cell), (writer, node)) {
                Some((previous, other))
                    if other != node && (previous.is_some() || writer.is_some()) =>
                {
                    Err(inconsistent(format!(
                        "node {node} and node {other} write one tree-local cell, which a \
                             detached node writes: it has one writer"
                    )))
                }
                _ => Ok(()),
            }
        };
        let output_cell = |node: Uuid, port: Uuid| {
            node_parameters_variables
                .get(&NodeParameterId {
                    node,
                    parameter: port,
                })
                .cloned()
        };

        for (at, node) in self.nodes.iter().enumerate() {
            let refuse = |why: String| {
                Err(inconsistent(format!(
                    "detached node {}: {} {why}",
                    node.id,
                    node.display(names)
                )))
            };
            signatures.extend(expression_call_signatures(
                || format!("detached node {}", node.id),
                node.arguments.values(),
                functions,
            )?);
            if is_interpreter_function_id(node.function) {
                return refuse("calls the interpreter module, which a tree does not call".into());
            }
            let signature = functions.get(&node.function);
            // The arguments in the function's parameter order, each
            // function's positions found once.
            let ordered = match signature {
                Some(signature) => {
                    let positions = positions.entry(node.function).or_insert_with(|| {
                        signature
                            .function
                            .parameter_ordering
                            .iter()
                            .enumerate()
                            .map(|(at, id)| (*id, at))
                            .collect()
                    });
                    crate::display::ordered_by(&node.arguments, positions)
                }
                None => crate::display::ordered(&node.arguments, None),
            };
            // The return first, then the mutable parameters in the function's
            // parameter order: outputs that share a destination write it in an
            // order that does not change from one load to the next.
            let mut ports = vec![_RET_PARAM_ID];
            if let Some(signature) = signature {
                for &(parameter, argument) in &ordered {
                    if parameter == _RET_PARAM_ID {
                        continue;
                    }
                    let Some(declared) = signature.function.parameters.get(&parameter) else {
                        return refuse(format!(
                            "names the parameter {parameter}, which {} does not have",
                            signature.function_name
                        ));
                    };
                    if declared.mutable {
                        if matches!(argument, Expression::Select { .. } | Expression::Call(_)) {
                            return refuse(format!(
                                "feeds the mutable parameter `{}` with {}: what the function \
                                 writes back needs a literal or a variable to hold it",
                                declared.name,
                                argument.display(names)
                            ));
                        }
                        ports.push(parameter);
                    }
                }
                // One copy per function, however many nodes call it.
                signatures
                    .entry(node.function)
                    .or_insert_with(|| signature.clone());
            }
            let mut written = Vec::new();
            let mut state = HashSet::new();
            for port in ports {
                if let Some(cell) = output_cell(node.id, port) {
                    claim(&cell, Some(at), node.id)?;
                    if port != _RET_PARAM_ID {
                        state.insert(cell_id(&cell));
                    }
                    let publish = self
                        .bindings
                        .get(&NodeParameterId {
                            node: node.id,
                            parameter: port,
                        })
                        .cloned();
                    written.push(Output {
                        port,
                        cell,
                        publish,
                        optional: output_optional(signature, &port),
                    });
                }
            }
            let mut nested_writes = false;
            for cell in call_writes(node, functions, variables, node_parameters_variables) {
                nested_writes = true;
                if matches!(cell, VariableCell::Stored(_)) {
                    return refuse(
                        "writes a store key back from a call in its arguments: a detached \
                         node writes the store only through its own outputs, after the tree \
                         ticks"
                            .into(),
                    );
                }
                claim(&cell, Some(at), node.id)?;
                state.insert(cell_id(&cell));
            }
            states.push(state);
            rewrites.push(
                nested_writes
                    || written
                        .iter()
                        .any(|output| output.port != _RET_PARAM_ID && output.publish.is_some()),
            );
            outputs.push(written);
            arguments.push(
                ordered
                    .into_iter()
                    .filter(|(parameter, _)| *parameter != _RET_PARAM_ID)
                    .map(|(parameter, argument)| {
                        let at = NodeParameterId {
                            node: node.id,
                            parameter,
                        };
                        Argument {
                            parameter,
                            optional: signature
                                .is_some_and(|f| is_optional(&f.function, &parameter)),
                            source: match argument {
                                Expression::Call(_) | Expression::Select { .. } => {
                                    Source::Computed(argument.clone())
                                }
                                _ => Source::Cell(
                                    self.bindings
                                        .get(&at)
                                        .or_else(|| node_parameters_variables.get(&at))
                                        .cloned(),
                                ),
                            },
                        }
                    })
                    .collect::<Vec<_>>(),
            );
            modules.push(signature.map(|signature| signature.module_id));
        }
        // What attached nodes write: their bound return, the mutable
        // parameters of the module functions they call, at any depth, and the
        // outputs of the tree's own nodes.
        for node in attached {
            claim_attached(node, functions, &output_cell, &mut claim)?;
            for cell in call_writes(node, functions, variables, node_parameters_variables) {
                claim(&cell, None, node.id)?;
            }
        }

        // An output bound to another node's input feeds it in place of its
        // argument, so that input reads a cell: a call or a selection computes
        // its value instead. A mutable parameter is known only now, after the
        // loader gave a port bound to a store key a sink's binding, so one
        // bound to an input naming a store key is refused: both bind a
        // tree-local variable instead.
        for (at, node) in self.nodes.iter().enumerate() {
            for Output { port, .. } in &outputs[at] {
                let Some(Expression::NodeArgument(input)) = node.arguments.get(port) else {
                    continue;
                };
                let refuse = |why: &str| {
                    Err(inconsistent(format!(
                        "detached node {}: {} binds an output to {}, {why}",
                        node.id,
                        node.display(names),
                        Expression::NodeArgument(input.clone()).display(names)
                    )))
                };
                // The input, through the ports that alias it: an input naming
                // another node's input shares its cell.
                let argument_of = |port: &NodeParameterId| {
                    self.nodes
                        .iter()
                        .chain(attached)
                        .find(|reader| reader.id == port.node)
                        .and_then(|reader| reader.arguments.get(&port.parameter))
                };
                let mut target = input;
                let mut computed = false;
                for _ in 0..=self.nodes.len() + attached.len() {
                    match argument_of(target) {
                        Some(Expression::NodeArgument(aliased)) => target = aliased,
                        Some(Expression::Call(_) | Expression::Select { .. }) => {
                            computed = true;
                            break;
                        }
                        _ => break,
                    }
                }
                if computed {
                    return refuse("an input a call or a selection computes, with no cell to feed");
                }
                // The store binding is the port's when the input is a tree
                // node's, and the input's own when it is a detached node's.
                let store_bound = [
                    &NodeParameterId {
                        node: node.id,
                        parameter: *port,
                    },
                    input,
                    target,
                ]
                .into_iter()
                .any(|bound| self.bindings.contains_key(bound));
                if *port != _RET_PARAM_ID && store_bound {
                    return refuse(
                        "an input that names a store key: bind both to a tree-local variable",
                    );
                }
            }
        }

        // A port read must be an output of a detached node. A port the return
        // or a mutable parameter is bound to directly is where the node
        // writes, not a read.
        for node in self.nodes.iter().chain(attached) {
            let signature = functions.get(&node.function);
            for (parameter, argument) in &node.arguments {
                let mutable = signature
                    .and_then(|signature| signature.function.parameters.get(parameter))
                    .is_some_and(|parameter| parameter.mutable);
                let writes = *parameter == _RET_PARAM_ID
                    || (mutable && matches!(argument, Expression::NodeArgument(_)));
                if writes {
                    continue;
                }
                let mut ports = Vec::new();
                ports_named(argument, &mut ports);
                for port in ports {
                    let Some(source) = self.ids.get(&port.node).map(|at| &self.nodes[*at]) else {
                        continue;
                    };
                    let Some(signature) = functions.get(&source.function) else {
                        continue;
                    };
                    let is_port = port.parameter == _RET_PARAM_ID
                        || signature
                            .function
                            .parameters
                            .get(&port.parameter)
                            .is_some_and(|parameter| parameter.mutable);
                    if !is_port {
                        return Err(inconsistent(format!(
                            "{} is read, and {} has no such output: an output is its return \
                             value or a mutable parameter",
                            argument.display(names),
                            signature.function_name
                        )));
                    }
                }
            }
        }

        // Who reads whom, from the cells each node's arguments read and the
        // cell each parameter reads, which another node's output may be bound
        // to. A detached node reads its own state; any other output of its own
        // it reads is feedback, which goes through a store key.
        let reads_of = |node: &Node, own: Option<usize>| {
            let mut cells = Vec::new();
            for (parameter, argument) in &node.arguments {
                // The return's binding is where the node writes, not a read.
                if *parameter != _RET_PARAM_ID {
                    cells_read(argument, variables, node_parameters_variables, &mut cells);
                    cells.extend(
                        node_parameters_variables
                            .get(&NodeParameterId {
                                node: node.id,
                                parameter: *parameter,
                            })
                            .cloned(),
                    );
                }
            }
            let mut read = Vec::new();
            for cell in &cells {
                let id = cell_id(cell);
                let Some((Some(writer), _)) = writers.get(&id) else {
                    continue;
                };
                if Some(*writer) != own {
                    read.push(*writer);
                } else if !states[*writer].contains(&id) {
                    return Err(inconsistent(format!(
                        "detached node {}: {} reads its own output: feedback goes through a \
                         store key",
                        node.id,
                        node.display(names)
                    )));
                }
            }
            read.sort_unstable();
            read.dedup();
            Ok(read)
        };
        let reads: Vec<Vec<usize>> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(at, node)| reads_of(node, Some(at)))
            .collect::<Result<_, _>>()?;
        let mut pulls = HashMap::new();
        let mut tree_reads = Vec::new();
        for node in attached {
            let read = reads_of(node, None)?;
            if !read.is_empty() {
                tree_reads.extend(read.iter().copied());
                pulls.insert(node.id, Rc::from(read));
            }
        }

        let fed = self
            .nodes
            .iter()
            .enumerate()
            .flat_map(|(at, node)| {
                node.arguments
                    .keys()
                    .filter(|parameter| **parameter != _RET_PARAM_ID)
                    .map(move |parameter| (at, node.id, *parameter))
            })
            .filter_map(|(at, node, parameter)| {
                let np = NodeParameterId { node, parameter };
                let cell = node_parameters_variables.get(&np)?;
                match writers.get(&cell_id(cell)) {
                    Some((Some(writer), _)) if *writer != at => Some(np),
                    _ => None,
                }
            })
            .collect::<crate::id_hash::IdSet<NodeParameterId>>();
        // A fed input reads the cell the output bound to it writes, even
        // when its argument names a store key.
        for (node, arguments) in self.nodes.iter().zip(&mut arguments) {
            for argument in arguments.iter_mut() {
                let np = NodeParameterId {
                    node: node.id,
                    parameter: argument.parameter,
                };
                if fed.contains(&np) && matches!(argument.source, Source::Cell(_)) {
                    argument.source = Source::Cell(node_parameters_variables.get(&np).cloned());
                }
            }
        }

        // Inputs before their readers, otherwise in the order given.
        let order = sorted(&reads, &self.nodes)?;

        let sink = |at: usize| outputs[at].iter().any(|output| output.publish.is_some());
        let closure = |starts: Vec<usize>| {
            let mut reached = HashSet::new();
            let mut stack = starts;
            while let Some(at) = stack.pop() {
                if reached.insert(at) {
                    stack.extend(reads[at].iter().copied());
                }
            }
            reached
        };
        let observed_ids = self.observed.borrow();
        let observed_at = |at: usize| observed_ids.contains(&self.nodes[at].id);
        let every_tick = closure(
            (0..self.nodes.len())
                .filter(|at| sink(*at) || observed_at(*at))
                .collect(),
        );
        let on_demand = closure(tree_reads);
        let mut plan = DetachedPlan::default();
        for at in &order {
            let id = self.nodes[*at].id;
            if every_tick.contains(at) {
                plan.every_tick.push(id);
            } else if on_demand.contains(at) {
                plan.on_demand.push(id);
            } else {
                plan.unused.push(id);
            }
        }
        let sinks = (0..self.nodes.len()).filter(|at| sink(*at)).collect();
        let observed = order
            .iter()
            .copied()
            .filter(|at| observed_at(*at) && !sink(*at))
            .collect();

        *self.lowered.borrow_mut() = Lowered {
            signatures,
            reads,
            cells_only: arguments
                .iter()
                .map(|arguments: &Vec<Argument>| {
                    arguments
                        .iter()
                        .all(|argument| matches!(argument.source, Source::Cell(_)))
                })
                .collect(),
            arguments,
            modules,
            rewrites,
            outputs,
            sinks,
            observed,
            pulls,
            plan,
        };
        Ok(())
    }

    /// Start a tick: every detached node is out of date until it is read.
    pub(crate) fn begin_tick(&self) {
        self.tick.set(self.tick.get() + 1);
    }

    /// Bring the detached nodes at `nodes` up to date, before a node that
    /// reads them ticks.
    pub(crate) fn pull(
        &self,
        nodes: &[usize],
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        for at in nodes {
            self.bring_up_to_date(*at, scope, caller)?;
        }
        Ok(())
    }

    /// Bring every sink up to date, then publish each one's store keys, in
    /// the order the tree lists them: no sink reads another's key of this tick.
    /// A sink that fails stops no other: the others are brought up to date and
    /// publish, and the tick reports the first failure. Then bring the
    /// observed nodes up to date, alike.
    pub(crate) fn tick_sinks(
        &self,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        let sinks = self.lowered.borrow().sinks.clone();
        let mut failure = None;
        for at in &sinks {
            if let Err(error) = self.bring_up_to_date(*at, scope, caller) {
                failure.get_or_insert(error);
            }
        }
        {
            let lowered = self.lowered.borrow();
            let caches = self.caches.borrow();
            for at in sinks.into_iter().filter(|at| caches[*at].failed.is_none()) {
                for output in &lowered.outputs[at] {
                    let value = caches[at]
                        .outputs
                        .iter()
                        .find(|(port, _)| *port == output.port)
                        .map(|(_, value)| value);
                    if let (Some(key), Some(value)) = (&output.publish, value) {
                        write_back(key, value.clone(), output.optional);
                    }
                }
            }
        }
        let observed = self.lowered.borrow().observed.clone();
        for at in observed {
            if let Err(error) = self.bring_up_to_date(at, scope, caller) {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Bring the detached node at `at` up to date for this tick, at most
    /// once: a second read in the tick reuses its result, or its failure.
    fn bring_up_to_date(
        &self,
        at: usize,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        if let Some(outcome) = self.this_tick(at) {
            return outcome;
        }
        // Lowering alone borrows `lowered` mutably, so the borrow is held
        // through the evaluation. Each node is evaluated once the nodes it
        // reads are: depth-first, iteratively, so a chain of any depth runs.
        let lowered = self.lowered.borrow();
        let mut stack = std::mem::take(&mut *self.stack.borrow_mut());
        stack.clear();
        stack.push((at, 0));
        let outcome = self.bring_up(&mut stack, &lowered, scope, caller);
        *self.stack.borrow_mut() = stack;
        outcome
    }

    /// Bring up to date the node at the bottom of `stack`, the nodes it
    /// reads first.
    fn bring_up(
        &self,
        stack: &mut Vec<(usize, usize)>,
        lowered: &Lowered,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        while let Some((node, next)) = stack.last_mut() {
            let node = *node;
            let failure = if let Some(read) = lowered.reads[node].get(*next).copied() {
                *next += 1;
                match self.this_tick(read) {
                    None => {
                        stack.push((read, 0));
                        continue;
                    }
                    Some(Ok(())) => continue,
                    Some(Err(failure)) => failure,
                }
            } else {
                match self.evaluate(node, lowered, scope, caller) {
                    Ok(()) => {
                        stack.pop();
                        self.mark(node, None);
                        continue;
                    }
                    Err(failure) => failure,
                }
            };
            // A failure fails every node waiting on it, for the rest of the
            // tick: a later read fails alike without calling again.
            let why = failure.to_string();
            for (waiting, _) in stack.iter() {
                self.mark(*waiting, Some(why.clone()));
            }
            return Err(failure);
        }
        Ok(())
    }

    /// The outcome of the detached node at `at` this tick, once evaluated:
    /// `None` while it is not.
    fn this_tick(&self, at: usize) -> Option<Result<(), BehaviorTreeError>> {
        let caches = self.caches.borrow();
        let cache = &caches[at];
        (cache.tick == self.tick.get()).then(|| match &cache.failed {
            None => Ok(()),
            Some(why) => Err(BehaviorTreeError::CallError(CallError::Generic {
                message: format!(
                    "detached node {} failed this tick: {why}",
                    self.nodes[at].id
                ),
            })),
        })
    }

    /// Mark the detached node at `at` evaluated this tick, with why it
    /// failed if it did.
    fn mark(&self, at: usize, failed: Option<String>) {
        let mut caches = self.caches.borrow_mut();
        caches[at].tick = self.tick.get();
        caches[at].failed = failed;
    }

    /// Evaluate the detached node at `at`, whose reads are up to date: call
    /// its function if its arguments changed.
    fn evaluate(
        &self,
        at: usize,
        lowered: &Lowered,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        let scope = Scope {
            calls: &lowered.signatures,
            ..scope
        };
        let node = &self.nodes[at];
        let outputs = &lowered.outputs[at];
        // A mutable parameter bound to a store key has the key's value as its
        // state: its port holds it too before the node reads its arguments,
        // so the node reading its own state through `@node.parameter` reads
        // the same value, the first time included.
        for output in outputs {
            if let (true, Some(key)) = (output.port != _RET_PARAM_ID, &output.publish) {
                output.cell.set_opt(key.get());
            }
        }
        // Arguments that all read cells compare with the last call's in
        // place: a node whose arguments did not change copies none.
        let mut known_changed = false;
        if lowered.cells_only[at] {
            let unchanged = match &self.caches.borrow()[at].arguments {
                Some(cached) => {
                    let unchanged = cached.len() == lowered.arguments[at].len()
                        && lowered.arguments[at]
                            .iter()
                            .zip(cached)
                            .all(|(argument, cached)| {
                                let Source::Cell(cell) = &argument.source else {
                                    return false;
                                };
                                holds_argument(cell.as_ref(), cached, argument.optional)
                            });
                    known_changed = !unchanged;
                    unchanged
                }
                None => false,
            };
            if unchanged {
                if lowered.rewrites[at] {
                    self.rewrite(at, outputs);
                }
                self.recorded(at, Activity::Cached);
                return Ok(());
            }
        }
        let mut values = Vec::with_capacity(lowered.arguments[at].len());
        for argument in &lowered.arguments[at] {
            // An argument reads its cell, as a module leaf's does; a call or
            // a selection is computed.
            let value = match &argument.source {
                Source::Computed(expression) => {
                    let np = NodeParameterId {
                        node: node.id,
                        parameter: argument.parameter,
                    };
                    Some(compute_expression(scope, expression, caller, &np)?)
                        .filter(|value| *value != Value::Unit)
                }
                Source::Cell(cell) => cell.as_ref().and_then(VariableCell::get),
            };
            values.push(argument_value(value, argument.optional));
        }
        let changed = known_changed || self.caches.borrow()[at].arguments.as_ref() != Some(&values);
        if !changed {
            if lowered.rewrites[at] {
                self.rewrite(at, outputs);
            }
            self.recorded(at, Activity::Cached);
            return Ok(());
        }
        let module_id = lowered.modules[at].ok_or(BehaviorTreeError::CallError(
            CallError::FunctionNotFound { id: node.function },
        ))?;
        // The arguments are lent to the call: a bridge answering in-process
        // copies none of them.
        let args: Vec<(Uuid, &Value)> = lowered.arguments[at]
            .iter()
            .zip(&values)
            .map(|(argument, value)| (argument.parameter, value))
            .collect();
        // A call counts whether or not it succeeds.
        self.caches.borrow_mut()[at].calls += 1;
        self.recorded(at, Activity::Called);
        let result = caller
            .arora_call_borrowed(Some(module_id), node.function, &args)
            .map_err(BehaviorTreeError::CallError)?;
        // A mutable parameter the call leaves as it was keeps the value it
        // was given; the ones it reports take their new value.
        let mut results: Vec<(Uuid, Value)> =
            Vec::with_capacity(outputs.len().max(1 + result.mutated.len()));
        if outputs.len() > 1 {
            for (argument, value) in lowered.arguments[at].iter().zip(&values) {
                if outputs
                    .iter()
                    .any(|output| output.port == argument.parameter)
                {
                    results.push((argument.parameter, value.clone()));
                }
            }
        }
        results.push((_RET_PARAM_ID, result.ret));
        for mutated in result.mutated {
            match results.iter_mut().find(|(port, _)| *port == mutated.id) {
                Some((_, value)) => *value = *mutated.value,
                None => results.push((mutated.id, *mutated.value)),
            }
        }
        // The ports now; the store keys after the tree ticks.
        for output in outputs {
            if let Some((_, value)) = results.iter().find(|(port, _)| *port == output.port) {
                write_back(&output.cell, value.clone(), output.optional);
            }
        }
        let mut caches = self.caches.borrow_mut();
        caches[at].arguments = Some(values);
        caches[at].outputs = results;
        Ok(())
    }

    /// Record that the node at `at` did `activity` this tick, while a trace
    /// runs.
    fn recorded(&self, at: usize, activity: Activity) {
        if self.record.is_on() {
            self.record.activity(self.nodes[at].id, activity);
        }
    }

    /// Write back the cached outputs of the node at `at`, whose cells a
    /// nested call of its arguments, or the store seeding a mutable port,
    /// may have written since its call.
    fn rewrite(&self, at: usize, outputs: &[Output]) {
        let caches = self.caches.borrow();
        for output in outputs {
            if let Some((_, value)) = caches[at]
                .outputs
                .iter()
                .find(|(port, _)| *port == output.port)
            {
                write_back(&output.cell, value.clone(), output.optional);
            }
        }
    }
}

/// An output port of a detached node: the tree-local cell it writes, the
/// store key it publishes to after the tree ticks, and whether it writes an
/// optional back.
struct Output {
    port: Uuid,
    cell: VariableCell,
    publish: Option<VariableCell>,
    optional: bool,
}

/// Whether `cell` holds what an argument of the last call passed,
/// `cached`, as [`argument_value`] makes it.
fn holds_argument(cell: Option<&VariableCell>, cached: &Value, optional: bool) -> bool {
    let matches = |value: Option<&Value>| match value {
        Some(value @ Value::Option(_)) => cached == value,
        Some(value) if optional => {
            matches!(cached, Value::Option(Some(inner)) if **inner == *value)
        }
        Some(value) => cached == value,
        None if optional => matches!(cached, Value::Option(None)),
        None => matches!(cached, Value::Unit),
    };
    match cell {
        Some(cell) => cell.with(matches),
        None => matches(None),
    }
}

/// Whether the output `port` of a call to `signature` writes an optional
/// back: its return type, or its parameter's.
fn output_optional(signature: Option<&arora_behavior::ModuleFunction>, port: &Uuid) -> bool {
    signature.is_some_and(|signature| {
        if *port == _RET_PARAM_ID {
            signature.function.return_ty.is_option()
        } else {
            is_optional(&signature.function, port)
        }
    })
}

/// Claim the tree-local cells an attached `node` writes: its bound return,
/// the mutable parameters of the module function it calls, and each argument
/// it computes from a call into the argument's cell.
fn claim_attached(
    node: &Node,
    functions: &FunctionIndex,
    output_cell: &impl Fn(Uuid, Uuid) -> Option<VariableCell>,
    claim: &mut impl FnMut(&VariableCell, Option<usize>, Uuid) -> Result<(), BehaviorTreeError>,
) -> Result<(), BehaviorTreeError> {
    let mut ports = Vec::new();
    if node.arguments.contains_key(&_RET_PARAM_ID) {
        ports.push(_RET_PARAM_ID);
    }
    ports.extend(native_writes(node.function).iter().copied());
    ports.extend(
        node.arguments
            .iter()
            .filter(|(_, argument)| matches!(argument, Expression::Call(_)))
            .map(|(parameter, _)| *parameter),
    );
    if let Some(signature) = functions.get(&node.function) {
        ports.extend(
            node.arguments
                .keys()
                .filter(|parameter| {
                    signature
                        .function
                        .parameters
                        .get(parameter)
                        .is_some_and(|parameter| parameter.mutable)
                })
                .copied(),
        );
    }
    for port in ports {
        if let Some(cell) = output_cell(node.id, port) {
            claim(&cell, None, node.id)?;
        }
    }
    Ok(())
}

/// The parameters a node of the tree's own vocabulary writes.
pub(crate) fn native_writes(function: Uuid) -> &'static [Uuid] {
    match function {
        UNPACK_FUNCTION_ID => &UNPACK_OUT_PARAM_IDS,
        SEQ_STAR_FUNCTION_ID => &[SEQ_STAR_CURRENT_INDEX_PARAM_ID],
        RUN_STATUS_FUNCTION_ID => &[RUN_STATUS_OUT_PARAM_ID, RUN_STATUS_LATCH_PARAM_ID],
        SET_STR_FUNCTION_ID => &[SET_STR_VARIABLE_PARAM_ID],
        UNSET_STR_FUNCTION_ID => &[UNSET_STR_VARIABLE_PARAM_ID],
        REGEX_MATCH_FUNCTION_ID => &[REGEX_MATCH_FIRST_MATCH_PARAM_ID],
        _ => &[],
    }
}

/// The cells the calls nested in `node`'s arguments may write back to, at
/// any depth, the calls computing a module, function or parameter id
/// included: the argument of each mutable parameter. When the function or
/// the parameter is computed, or the function is not in `functions`, which
/// parameters are mutable is not known, so every cell the call's arguments
/// name counts.
fn call_writes(
    node: &Node,
    functions: &FunctionIndex,
    variables: &HashMap<Uuid, VariableCell>,
    node_parameters_variables: &crate::id_hash::IdMap<NodeParameterId, VariableCell>,
) -> Vec<VariableCell> {
    fn walk(
        expression: &Expression,
        functions: &FunctionIndex,
        variables: &HashMap<Uuid, VariableCell>,
        node_parameters_variables: &crate::id_hash::IdMap<NodeParameterId, VariableCell>,
        found: &mut Vec<VariableCell>,
    ) {
        match expression {
            Expression::Select { source, .. } => walk(
                source,
                functions,
                variables,
                node_parameters_variables,
                found,
            ),
            Expression::Call(call) => {
                let signature = stated_id(&call.function).and_then(|id| functions.get(&id));
                for identifier in [&call.module, &call.function] {
                    walk(
                        identifier,
                        functions,
                        variables,
                        node_parameters_variables,
                        found,
                    );
                }
                for (parameter, argument) in &call.arguments {
                    let mutable = match (signature, stated_id(parameter)) {
                        (Some(signature), Some(id)) => signature
                            .function
                            .parameters
                            .get(&id)
                            .is_some_and(|parameter| parameter.mutable),
                        _ => true,
                    };
                    walk(
                        parameter,
                        functions,
                        variables,
                        node_parameters_variables,
                        found,
                    );
                    if mutable {
                        found.extend(match argument {
                            Expression::Variable(cell) => Some(cell.clone()),
                            Expression::VariableId(id) => variables.get(id).cloned(),
                            Expression::NodeArgument(at) => {
                                node_parameters_variables.get(at).cloned()
                            }
                            _ => None,
                        });
                    }
                    walk(
                        argument,
                        functions,
                        variables,
                        node_parameters_variables,
                        found,
                    );
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for argument in node.arguments.values() {
        walk(
            argument,
            functions,
            variables,
            node_parameters_variables,
            &mut found,
        );
    }
    found
}

/// Every node port `expression` names, at any depth.
fn ports_named(expression: &Expression, found: &mut Vec<NodeParameterId>) {
    match expression {
        Expression::NodeArgument(port) => found.push(port.clone()),
        Expression::Select { source, .. } => ports_named(source, found),
        Expression::Call(call) => {
            ports_named(&call.module, found);
            ports_named(&call.function, found);
            for (parameter, argument) in &call.arguments {
                ports_named(parameter, found);
                ports_named(argument, found);
            }
        }
        _ => {}
    }
}

/// Depth-first: push `at` after every node it reads. A node met again on the
/// current `path` closes a cycle.
/// The detached nodes in evaluation order: depth-first, each after every
/// node it reads, otherwise in the order given. A node met again on the path
/// that leads to it closes a cycle. Iterative, so a chain of any depth sorts.
fn sorted(reads: &[Vec<usize>], nodes: &[Rc<Node>]) -> Result<Vec<usize>, BehaviorTreeError> {
    let mut sorted = Vec::with_capacity(nodes.len());
    let mut done = vec![false; nodes.len()];
    let mut on_path = vec![false; nodes.len()];
    // The path: each node on it, and the index of the next read to visit.
    let mut path: Vec<(usize, usize)> = Vec::new();
    for start in 0..nodes.len() {
        if done[start] {
            continue;
        }
        path.push((start, 0));
        on_path[start] = true;
        while let Some((at, next)) = path.last_mut() {
            let at = *at;
            if let Some(read) = reads[at].get(*next).copied() {
                *next += 1;
                if done[read] {
                    continue;
                }
                if on_path[read] {
                    let from = path
                        .iter()
                        .position(|(on, _)| *on == read)
                        .expect("a node on the path is in it");
                    let cycle: Vec<String> = path[from..]
                        .iter()
                        .map(|(at, _)| nodes[*at].id.to_string())
                        .collect();
                    return Err(inconsistent(format!(
                        "the detached nodes {} read each other in a cycle: feed a value back \
                         through a store key",
                        cycle.join(", ")
                    )));
                }
                on_path[read] = true;
                path.push((read, 0));
            } else {
                path.pop();
                on_path[at] = false;
                done[at] = true;
                sorted.push(at);
            }
        }
    }
    Ok(sorted)
}
