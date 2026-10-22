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
//! cm: clamp(x: @raw, lo: 0f64)
//! level: {face/level} = add(a: @cm, b: 1f64)
//! ```
//!
//! # What reads a detached node
//!
//! Nothing marks a read. A node reads a detached node when one of its
//! arguments reads a tree-local cell the detached node writes: its return
//! value, as `@node` ([`Expression::NodeArgument`] on `_ret`) or through the
//! variable `_ret` is bound to, or one of its mutable parameters, as
//! `@node.parameter` or through its variable. Lowering finds the writer of
//! every such cell, and gives each node the detached nodes it reads. A
//! detached node's return value is a port even when `_ret` is not bound. An
//! output bound to another node's input, as `_ret: @reader.x`, writes the cell
//! that input reads, so the reader reads the detached node.
//!
//! A store key is not such a cell. A port bound to a store key keeps a
//! tree-local cell of its own, which `@node` reads, and publishes to the key
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
//! tick**, after the tree has ticked, even when the tree failed. Once every sink is up to date, each
//! publishes its keys, in the order the tree lists them, even when its result
//! comes from its cache: a sink never reads another's key of the same tick,
//! and of two sinks of one key, the later listed wins, as the later writer
//! does in a node graph. A tree built from the shared `Graph`, which keeps no
//! order among its nodes, lists them by id after the root.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use arora_behavior::FunctionIndex;
use arora_types::call::{Call, CallBridge, CallError};
use arora_types::value::{StructureField, Value};
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
    /// The sinks and the nodes they read: brought up to date at every tick.
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
    /// The store keys the ports of detached nodes publish to, by port; the
    /// port itself is the tree-local cell the node's argument has.
    bindings: HashMap<NodeParameterId, VariableCell>,
    /// What lowering found: who reads whom, and what each node writes.
    lowered: RefCell<Lowered>,
    caches: RefCell<Vec<Cache>>,
    /// The current tick: a node whose cache is of this tick is up to date.
    tick: Cell<u64>,
}

#[derive(Default)]
struct Lowered {
    /// The functions the detached nodes call, and those their nested calls
    /// state.
    signatures: FunctionIndex,
    /// By node: the detached nodes it reads, by position.
    reads: Vec<Vec<usize>>,
    /// By node: its arguments in signature order, `_ret` aside.
    arguments: Vec<Vec<Uuid>>,
    /// By node: its outputs, each a port, the tree-local cell it writes, and
    /// the store key it publishes to after the tree ticks.
    outputs: Vec<Vec<(Uuid, VariableCell, Option<VariableCell>)>>,
    /// The sinks, by position, in the order the tree lists them.
    sinks: Vec<usize>,
    /// By attached node: the detached nodes it reads, by position.
    pulls: HashMap<Uuid, Rc<[usize]>>,
    plan: DetachedPlan,
}

#[derive(Default, Clone)]
struct Cache {
    /// The tick the node was last brought up to date at.
    tick: u64,
    /// The arguments of its last call.
    arguments: Option<Vec<Value>>,
    /// Its outputs after its last call, by port.
    outputs: HashMap<Uuid, Value>,
    /// How many times it called its function.
    calls: u64,
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
    node_parameters_variables: &HashMap<NodeParameterId, VariableCell>,
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

impl Detached {
    /// The detached nodes of `nodes`, in their order: the calls to a module
    /// function, with no children, that `attached` does not hold.
    pub(crate) fn select(nodes: &[Rc<Node>], attached: &HashSet<Uuid>) -> Vec<Rc<Node>> {
        nodes
            .iter()
            .filter(|node| {
                !attached.contains(&node.id)
                    && !is_native(node.function)
                    && node
                        .children
                        .as_ref()
                        .is_none_or(|children| children.is_empty())
            })
            .cloned()
            .collect()
    }

    /// The detached `nodes`, whose ports publish to the store keys
    /// `bindings` name.
    pub(crate) fn new(
        nodes: Vec<Rc<Node>>,
        bindings: HashMap<NodeParameterId, VariableCell>,
    ) -> Self {
        let caches = RefCell::new(vec![Cache::default(); nodes.len()]);
        Self {
            nodes,
            bindings,
            lowered: RefCell::default(),
            caches,
            tick: Cell::new(1),
        }
    }

    pub(crate) fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            bindings: HashMap::new(),
            lowered: RefCell::default(),
            caches: RefCell::default(),
            tick: Cell::new(1),
        }
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
        let at = self.nodes.iter().position(|node| node.id == id)?;
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
    /// detached node writes only through its outputs; a read of a mutable
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
        node_parameters_variables: &HashMap<NodeParameterId, VariableCell>,
    ) -> Result<(), BehaviorTreeError> {
        let names = Names::functions(functions);
        let mut signatures = FunctionIndex::new();
        let mut arguments = Vec::with_capacity(self.nodes.len());
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
                &format!("detached node {}", node.id),
                node.arguments.values(),
                functions,
            )?);
            if is_interpreter_function_id(node.function) {
                return refuse("calls the interpreter module, which a tree does not call".into());
            }
            let signature = functions.get(&node.function);
            let mut ports = vec![_RET_PARAM_ID];
            if let Some(signature) = signature {
                for (parameter, argument) in &node.arguments {
                    if *parameter == _RET_PARAM_ID {
                        continue;
                    }
                    let Some(declared) = signature.function.parameters.get(parameter) else {
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
                        ports.push(*parameter);
                    }
                }
                signatures.insert(node.function, signature.clone());
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
                    written.push((port, cell, publish));
                }
            }
            for cell in call_writes(node, functions, variables, node_parameters_variables) {
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
            outputs.push(written);
            arguments.push(
                crate::display::ordered(&node.arguments, signature)
                    .into_iter()
                    .map(|(parameter, _)| parameter)
                    .filter(|parameter| *parameter != _RET_PARAM_ID)
                    .collect::<Vec<_>>(),
            );
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

        // A port read must be an output of a detached node. A port the return
        // is bound to is where the node writes, not a read.
        for node in self.nodes.iter().chain(attached) {
            for (parameter, argument) in &node.arguments {
                if *parameter == _RET_PARAM_ID {
                    continue;
                }
                let mut ports = Vec::new();
                ports_named(argument, &mut ports);
                for port in ports {
                    let Some(source) = self.nodes.iter().find(|node| node.id == port.node) else {
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

        // Inputs before their readers, otherwise in the order given.
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut done = HashSet::new();
        for at in 0..self.nodes.len() {
            visit(
                at,
                &reads,
                &self.nodes,
                &mut done,
                &mut Vec::new(),
                &mut order,
            )?;
        }

        let sink = |at: usize| outputs[at].iter().any(|(_, _, publish)| publish.is_some());
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
        let every_tick = closure((0..self.nodes.len()).filter(|at| sink(*at)).collect());
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

        *self.lowered.borrow_mut() = Lowered {
            signatures,
            reads,
            arguments,
            outputs,
            sinks,
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
    pub(crate) fn tick_sinks(
        &self,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        let sinks = self.lowered.borrow().sinks.clone();
        for at in &sinks {
            self.bring_up_to_date(*at, scope, caller)?;
        }
        let lowered = self.lowered.borrow();
        let caches = self.caches.borrow();
        for at in sinks {
            let signature = lowered.signatures.get(&self.nodes[at].function);
            for (port, _, publish) in &lowered.outputs[at] {
                if let (Some(key), Some(value)) = (publish, caches[at].outputs.get(port)) {
                    write_back(key, value.clone(), output_optional(signature, port));
                }
            }
        }
        Ok(())
    }

    fn bring_up_to_date(
        &self,
        at: usize,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        if self.caches.borrow()[at].tick == self.tick.get() {
            return Ok(());
        }
        let reads = self.lowered.borrow().reads[at].clone();
        for read in reads {
            self.bring_up_to_date(read, scope, caller)?;
        }
        let lowered = self.lowered.borrow();
        let scope = Scope {
            calls: &lowered.signatures,
            ..scope
        };
        let node = &self.nodes[at];
        let signature = lowered.signatures.get(&node.function);
        let mut values = Vec::with_capacity(lowered.arguments[at].len());
        for parameter in &lowered.arguments[at] {
            let argument = &node.arguments[parameter];
            let np = NodeParameterId {
                node: node.id,
                parameter: *parameter,
            };
            // An argument reads its cell, as a module leaf's does; a call or
            // a selection is computed.
            let value = match argument {
                Expression::Call(_) | Expression::Select { .. } => {
                    Some(compute_expression(scope, argument, caller, &np)?)
                        .filter(|value| *value != Value::Unit)
                }
                _ => self
                    .bindings
                    .get(&np)
                    .or_else(|| scope.node_parameters_variables.get(&np))
                    .and_then(VariableCell::get),
            };
            let optional = signature.is_some_and(|f| is_optional(&f.function, parameter));
            values.push(argument_value(value, optional));
        }
        let changed = self.caches.borrow()[at].arguments.as_ref() != Some(&values);
        if changed {
            let signature =
                signature.ok_or(BehaviorTreeError::CallError(CallError::FunctionNotFound {
                    id: node.function,
                }))?;
            let args = lowered.arguments[at]
                .iter()
                .zip(&values)
                .map(|(parameter, value)| StructureField {
                    id: *parameter,
                    value: Box::new(value.clone()),
                })
                .collect();
            let result = caller
                .arora_call(Call {
                    module_id: Some(signature.module_id),
                    id: node.function,
                    args,
                })
                .map_err(BehaviorTreeError::CallError)?;
            let mut outputs = HashMap::from([(_RET_PARAM_ID, result.ret)]);
            for mutated in result.mutated {
                outputs.insert(mutated.id, *mutated.value);
            }
            // The ports now; the store keys after the tree ticks.
            for (port, cell, _) in &lowered.outputs[at] {
                if let Some(value) = outputs.get(port) {
                    write_back(cell, value.clone(), output_optional(Some(signature), port));
                }
            }
            let mut caches = self.caches.borrow_mut();
            caches[at].arguments = Some(values);
            caches[at].outputs = outputs;
            caches[at].calls += 1;
        }
        self.caches.borrow_mut()[at].tick = self.tick.get();
        Ok(())
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
    node_parameters_variables: &HashMap<NodeParameterId, VariableCell>,
) -> Vec<VariableCell> {
    fn walk(
        expression: &Expression,
        functions: &FunctionIndex,
        variables: &HashMap<Uuid, VariableCell>,
        node_parameters_variables: &HashMap<NodeParameterId, VariableCell>,
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
fn visit(
    at: usize,
    reads: &[Vec<usize>],
    nodes: &[Rc<Node>],
    done: &mut HashSet<usize>,
    path: &mut Vec<usize>,
    sorted: &mut Vec<usize>,
) -> Result<(), BehaviorTreeError> {
    if done.contains(&at) {
        return Ok(());
    }
    if let Some(start) = path.iter().position(|on| *on == at) {
        let cycle: Vec<String> = path[start..]
            .iter()
            .map(|at| nodes[*at].id.to_string())
            .collect();
        return Err(inconsistent(format!(
            "the detached nodes {} read each other in a cycle: feed a value back through a \
             store key",
            cycle.join(", ")
        )));
    }
    path.push(at);
    for read in &reads[at] {
        visit(*read, reads, nodes, done, path, sorted)?;
    }
    path.pop();
    done.insert(at);
    sorted.push(at);
    Ok(())
}
