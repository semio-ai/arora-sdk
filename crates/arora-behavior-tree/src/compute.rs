//! Compute nodes: calls to module functions held beside the tree, not in it.
//!
//! A compute node is one call: a function, one argument per parameter, and
//! optionally a variable its result is written to. Its arguments are
//! expressions like a tree node's: literals, variables, nested calls, and
//! [`Expression::Computed`] references to another compute node's output. The
//! references join the compute nodes into a graph, which the tree reads
//! through the same references in its own nodes' arguments.
//!
//! ```text
//! #raw = mul(a: {sensor/distance}, b: 100f64)
//! #cm = clamp(x: #raw, lo: 0f64)
//! #level = add(a: #cm, b: 1f64) -> {face/level}
//! ```
//!
//! # Evaluation
//!
//! A compute node is evaluated **on demand**: when a tree node that ticks
//! reads it, or when a node it feeds is evaluated. A node that nothing reads
//! is not evaluated, unless it writes a store key ([`ComputePlan`]).
//!
//! - **Once per tick.** The first read of a node in a tick brings it up to
//!   date. Every later read in the same tick reuses that result, however many
//!   readers it has. A variable the tree writes after that first read reaches
//!   the node at the next tick.
//! - **Only when its inputs changed.** Bringing a node up to date evaluates its
//!   arguments, which brings the nodes it reads up to date first. The node
//!   calls its function only when the argument values differ from those of its
//!   last call. A settled graph makes no calls. A changed input calls the
//!   nodes downstream of it, and no others.
//! - **A volatile node calls its function at every tick it is read.** A
//!   function whose result does not follow from its arguments, such as a
//!   sensor, a clock or a random source, is volatile.
//! - **A stateful function keeps its state in a mutable parameter.** A literal
//!   argument to it is the node's own cell, which starts from the literal and
//!   holds what the function writes back. A changed state is a changed input,
//!   so the node is called until its state settles.
//!
//! # Outputs
//!
//! A node's ports are its return value, read as `#node`, and the value each of
//! its mutable parameters had after its last call, read as `#node.parameter`.
//!
//! A node whose `output` is a **store key** is a sink. A sink is brought up to
//! date at **every tick**, after the tree has ticked, whether or not the tree
//! reads it, and writes its result to its key. The write happens every tick,
//! even when the result comes from the cache, so the nodes' order decides
//! between two sinks of one key: the later one wins, as the later writer does
//! in a node graph. A node whose `output` is a tree-local variable writes it
//! when it is brought up to date, and is evaluated only on demand.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use arora_behavior::FunctionIndex;
use arora_types::call::{Call, CallBridge, CallError};
use arora_types::value::{StructureField, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::display::Names;
use crate::error::BehaviorTreeError;
use crate::schema::{Expression, NodeParameterId, _RET_PARAM_ID};
use crate::variable::{VariableCell, VariableResolver};
use crate::{
    argument_value, bind_variables, compute_expression, expression_call_signatures,
    is_interpreter_function_id, is_optional, variable_cell, with_call_site_cells, write_back,
    written_cell, Scope,
};

/// One call to a module function, held beside the tree. See the
/// [module docs](self).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ComputeNode {
    /// The node's id, which [`Expression::Computed`] references name.
    pub id: Uuid,
    /// The function it calls, under the module the function index names.
    pub function: Uuid,
    /// One argument per parameter, by parameter id, evaluated in this order.
    #[serde(default)]
    pub inputs: Vec<(Uuid, Expression)>,
    /// The variable its return value is written to. A store key makes the
    /// node a sink, brought up to date at every tick.
    #[serde(default)]
    pub output: Option<Uuid>,
    /// Call the function at every tick the node is read, even when its
    /// arguments did not change.
    #[serde(default)]
    pub volatile: bool,
}

/// How a tree's compute nodes are evaluated, from what reads them. Each list
/// holds node ids in the graph's order, inputs before their readers, and each
/// node is in exactly one of them.
#[derive(Debug, Default, PartialEq)]
pub struct ComputePlan {
    /// The sinks and the nodes they read: brought up to date at every tick.
    pub every_tick: Vec<Uuid>,
    /// The nodes the tree reads and the nodes they read, apart from the
    /// above: brought up to date at a tick only when a tree node that reads
    /// them ticks.
    pub on_demand: Vec<Uuid>,
    /// The nodes nothing reads and that write no store key: never evaluated.
    pub unused: Vec<Uuid>,
}

/// A tree's compute nodes, ready to evaluate.
pub(crate) struct ComputeGraph {
    /// In an order where each node comes after the nodes it reads.
    slots: Vec<Slot>,
    index: HashMap<Uuid, usize>,
    /// Every reference to a compute node's port, from the tree or another
    /// compute node: checked against the functions when lowering.
    references: Vec<NodeParameterId>,
    plan: ComputePlan,
    /// The current tick: a node whose cache is of this tick is up to date.
    tick: Cell<u64>,
    /// The functions the nodes call, resolved when the tree is lowered.
    signatures: RefCell<FunctionIndex>,
}

struct Slot {
    /// The node as it was given, for display.
    authored: ComputeNode,
    /// Its arguments, a literal held in a cell of its own.
    inputs: Vec<(Uuid, Expression)>,
    output: Option<VariableCell>,
    sink: bool,
    cache: RefCell<Cache>,
}

#[derive(Default)]
struct Cache {
    /// The tick the node was last brought up to date at.
    tick: u64,
    /// The arguments of its last call.
    arguments: Option<Vec<Value>>,
    /// Its ports after its last call: the return value under `_ret`, and each
    /// mutated parameter under its id.
    outputs: HashMap<Uuid, Value>,
    /// How many times it called its function.
    calls: u64,
}

/// Every reference to a compute node's port in `expression`, at any depth.
pub(crate) fn references_in(expression: &Expression, found: &mut Vec<NodeParameterId>) {
    match expression {
        Expression::Computed(port) => found.push(port.clone()),
        Expression::Select { source, .. } => references_in(source, found),
        Expression::Call(call) => {
            references_in(&call.module, found);
            references_in(&call.function, found);
            for (parameter, argument) in &call.arguments {
                references_in(parameter, found);
                references_in(argument, found);
            }
        }
        _ => {}
    }
}

fn inconsistent(message: String) -> BehaviorTreeError {
    BehaviorTreeError::InconsistentTreeError { message }
}

impl ComputeGraph {
    pub(crate) fn empty() -> Self {
        Self::build(
            Vec::new(),
            Vec::new(),
            &mut HashMap::new(),
            &|_| None,
            &HashMap::new(),
        )
        .expect("an empty graph builds")
    }

    /// Build the graph of `nodes`, which the tree reads through `tree_reads`.
    ///
    /// Refuses two nodes of one id, a reference to a node that does not exist,
    /// a node argument (a compute node is not in the tree), and a cycle: a
    /// value fed back from one tick to the next goes through a store key.
    pub(crate) fn build(
        nodes: Vec<ComputeNode>,
        tree_reads: Vec<NodeParameterId>,
        variables: &mut HashMap<Uuid, VariableCell>,
        resolver: &VariableResolver,
        names: &HashMap<Uuid, String>,
    ) -> Result<Self, BehaviorTreeError> {
        let mut by_id: HashMap<Uuid, ComputeNode> = HashMap::new();
        let mut order: Vec<Uuid> = Vec::new();
        for node in nodes {
            order.push(node.id);
            if let Some(node) = by_id.insert(node.id, node) {
                return Err(inconsistent(format!("two compute nodes are {}", node.id)));
            }
        }
        let mut references = tree_reads.clone();
        let mut reads: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for id in &order {
            let mut found = Vec::new();
            for (_, input) in &by_id[id].inputs {
                if matches!(input, Expression::NodeArgument(_)) {
                    return Err(inconsistent(format!(
                        "compute node {id} reads a tree node's argument: it reads the tree \
                         through variables"
                    )));
                }
                references_in(input, &mut found);
            }
            reads.insert(*id, found.iter().map(|port| port.node).collect());
            references.extend(found);
        }
        if let Some(port) = references
            .iter()
            .find(|port| !by_id.contains_key(&port.node))
        {
            return Err(inconsistent(format!(
                "#{} is read, and is not a compute node",
                port.node
            )));
        }

        // Inputs before their readers, otherwise in the order given: a
        // depth-first walk from each node in turn.
        let mut sorted = Vec::with_capacity(order.len());
        let mut done = HashSet::new();
        for id in &order {
            let mut path = Vec::new();
            visit(*id, &reads, &mut done, &mut path, &mut sorted)?;
        }

        let mut slots = Vec::with_capacity(sorted.len());
        for id in sorted {
            let authored = by_id.remove(&id).expect("each node is sorted once");
            for (_, input) in &authored.inputs {
                bind_variables(input, variables, resolver, names);
            }
            let output = authored
                .output
                .map(|variable| variable_cell(variable, variables, resolver, names));
            let sink = matches!(output, Some(VariableCell::Stored(_)));
            let inputs = authored
                .inputs
                .iter()
                .map(|(parameter, input)| {
                    let input = match input.clone() {
                        Expression::Value(value) => {
                            Expression::Variable(VariableCell::local_with(value))
                        }
                        input => with_call_site_cells(input),
                    };
                    (*parameter, input)
                })
                .collect();
            slots.push(Slot {
                authored,
                inputs,
                output,
                sink,
                cache: RefCell::default(),
            });
        }
        let index: HashMap<Uuid, usize> = slots
            .iter()
            .enumerate()
            .map(|(at, slot)| (slot.authored.id, at))
            .collect();

        let closure = |starts: Vec<Uuid>| {
            let mut reached = HashSet::new();
            let mut stack = starts;
            while let Some(id) = stack.pop() {
                if reached.insert(id) {
                    stack.extend(reads[&id].iter().copied());
                }
            }
            reached
        };
        let every_tick = closure(
            slots
                .iter()
                .filter(|slot| slot.sink)
                .map(|slot| slot.authored.id)
                .collect(),
        );
        let on_demand = closure(tree_reads.iter().map(|port| port.node).collect());
        let mut plan = ComputePlan::default();
        for slot in &slots {
            let id = slot.authored.id;
            if every_tick.contains(&id) {
                plan.every_tick.push(id);
            } else if on_demand.contains(&id) {
                plan.on_demand.push(id);
            } else {
                plan.unused.push(id);
            }
        }

        Ok(Self {
            slots,
            index,
            references,
            plan,
            tick: Cell::new(1),
            signatures: RefCell::default(),
        })
    }

    pub(crate) fn plan(&self) -> &ComputePlan {
        &self.plan
    }

    /// How many times the node `id` called its function.
    pub(crate) fn calls(&self, id: Uuid) -> Option<u64> {
        self.index
            .get(&id)
            .map(|at| self.slots[*at].cache.borrow().calls)
    }

    /// The nodes as given, in evaluation order.
    pub(crate) fn nodes(&self) -> impl Iterator<Item = &ComputeNode> {
        self.slots.iter().map(|slot| &slot.authored)
    }

    /// Resolve the nodes' functions against `functions`.
    ///
    /// Refuses, naming the node: a call to the interpreter module; an argument
    /// for a parameter the function does not have; a mutable parameter fed by
    /// a selection, a call or another node's port, none of which has a cell
    /// to hold what the function writes back; and a reference to a port that
    /// is neither the return value nor a mutable parameter. A function
    /// `functions` does not have fails the node when it is evaluated, as a
    /// module leaf fails.
    pub(crate) fn lower(&self, functions: &FunctionIndex) -> Result<(), BehaviorTreeError> {
        let names = Names::functions(functions);
        let mut signatures = FunctionIndex::new();
        for slot in &self.slots {
            let node = &slot.authored;
            let refuse = |why: String| {
                Err(inconsistent(format!(
                    "compute node {}: {} {why}",
                    node.id,
                    node.display(names)
                )))
            };
            signatures.extend(expression_call_signatures(
                &format!("compute node {}", node.id),
                node.inputs.iter().map(|(_, input)| input),
                functions,
            )?);
            if is_interpreter_function_id(node.function) {
                return refuse("calls the interpreter module, which a tree does not call".into());
            }
            let Some(signature) = functions.get(&node.function) else {
                continue;
            };
            for (parameter, input) in &node.inputs {
                let Some(declared) = signature.function.parameters.get(parameter) else {
                    return refuse(format!(
                        "names the parameter {parameter}, which {} does not have",
                        signature.function_name
                    ));
                };
                if declared.mutable
                    && matches!(
                        input,
                        Expression::Select { .. } | Expression::Call(_) | Expression::Computed(_)
                    )
                {
                    return refuse(format!(
                        "feeds the mutable parameter `{}` with {}: what the function writes \
                         back needs a literal or a variable to hold it",
                        declared.name,
                        input.display(names)
                    ));
                }
            }
            signatures.insert(node.function, signature.clone());
        }
        for port in &self.references {
            let node = &self.slots[self.index[&port.node]].authored;
            let Some(signature) = functions.get(&node.function) else {
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
                    "{} is read, and {} has no such output: a port is its return value \
                     or a mutable parameter",
                    Expression::Computed(port.clone()).display(names),
                    signature.function_name
                )));
            }
        }
        *self.signatures.borrow_mut() = signatures;
        Ok(())
    }

    /// Start a tick: every node is out of date until it is read.
    pub(crate) fn begin_tick(&self) {
        self.tick.set(self.tick.get() + 1);
    }

    /// The value of `port`, its node brought up to date first.
    pub(crate) fn read(
        &self,
        port: &NodeParameterId,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<Value, BehaviorTreeError> {
        let at = *self
            .index
            .get(&port.node)
            .ok_or_else(|| inconsistent(format!("#{} is not a compute node", port.node)))?;
        self.bring_up_to_date(at, scope, caller)?;
        Ok(self.slots[at]
            .cache
            .borrow()
            .outputs
            .get(&port.parameter)
            .cloned()
            .unwrap_or(Value::Unit))
    }

    /// Bring every sink up to date, in order, and write its result to its key.
    pub(crate) fn tick_sinks(
        &self,
        scope: Scope,
        caller: &mut dyn CallBridge,
    ) -> Result<(), BehaviorTreeError> {
        for (at, slot) in self.slots.iter().enumerate() {
            if !slot.sink {
                continue;
            }
            self.bring_up_to_date(at, scope, caller)?;
            if let Some(output) = &slot.output {
                let value = slot.cache.borrow().outputs.get(&_RET_PARAM_ID).cloned();
                output.set_opt(value.filter(|value| *value != Value::Unit));
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
        let slot = &self.slots[at];
        if slot.cache.borrow().tick == self.tick.get() {
            return Ok(());
        }
        let signatures = self.signatures.borrow();
        let scope = Scope {
            calls: &signatures,
            ..scope
        };
        let node = &slot.authored;
        let signature = signatures.get(&node.function);
        let mut values = Vec::with_capacity(slot.inputs.len());
        let mut written = Vec::new();
        for (parameter, input) in &slot.inputs {
            let at = NodeParameterId {
                node: node.id,
                parameter: *parameter,
            };
            let value = compute_expression(scope, input, caller, &at)?;
            let optional = signature.is_some_and(|f| is_optional(&f.function, parameter));
            if let Some(cell) = written_cell(scope, input) {
                written.push((*parameter, cell, optional));
            }
            values.push(argument_value(
                Some(value).filter(|value| *value != Value::Unit),
                optional,
            ));
        }
        let changed = node.volatile || slot.cache.borrow().arguments.as_ref() != Some(&values);
        if changed {
            let signature =
                signature.ok_or(BehaviorTreeError::CallError(CallError::FunctionNotFound {
                    id: node.function,
                }))?;
            let args = slot
                .inputs
                .iter()
                .zip(&values)
                .map(|((parameter, _), value)| StructureField {
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
                if let Some((_, cell, optional)) = written
                    .iter()
                    .find(|(parameter, ..)| *parameter == mutated.id)
                {
                    write_back(cell, (*mutated.value).clone(), *optional);
                }
                outputs.insert(mutated.id, *mutated.value);
            }
            let mut cache = slot.cache.borrow_mut();
            cache.arguments = Some(values);
            cache.outputs = outputs;
            cache.calls += 1;
        }
        slot.cache.borrow_mut().tick = self.tick.get();
        if let (false, Some(output)) = (slot.sink, &slot.output) {
            let value = slot.cache.borrow().outputs.get(&_RET_PARAM_ID).cloned();
            output.set_opt(value.filter(|value| *value != Value::Unit));
        }
        Ok(())
    }
}

/// Depth-first: push `id` after every node it reads. A node met again on the
/// current `path` closes a cycle.
fn visit(
    id: Uuid,
    reads: &HashMap<Uuid, Vec<Uuid>>,
    done: &mut HashSet<Uuid>,
    path: &mut Vec<Uuid>,
    sorted: &mut Vec<Uuid>,
) -> Result<(), BehaviorTreeError> {
    if done.contains(&id) {
        return Ok(());
    }
    if let Some(start) = path.iter().position(|on| *on == id) {
        let cycle: Vec<String> = path[start..].iter().map(|id| format!("#{id}")).collect();
        return Err(inconsistent(format!(
            "the compute nodes {} read each other in a cycle: feed a value back through a \
             store key",
            cycle.join(", ")
        )));
    }
    path.push(id);
    for read in &reads[&id] {
        visit(*read, reads, done, path, sorted)?;
    }
    path.pop();
    done.insert(id);
    sorted.push(id);
    Ok(())
}
