//! A node graph run on the behavior tree, behind the reference evaluator's
//! surface: stage inputs and set the clock, evaluate, then read every
//! node's ports and the writes of its `output` nodes.
//!
//! [`TreeRuntime`] holds the graph's own store: the staged inputs, the clock
//! and the keys the graph writes. It [imports](crate::import) the graph,
//! builds the tree over that store, and lowers it onto a bridge running the
//! [node functions](crate::functions) in-process. An evaluation is one tick
//! of the tree.
//!
//! What it evaluates is a choice ([`Evaluation`]): every node at every tick,
//! as the reference evaluator does, so every port can be read and every
//! node's error surfaces; or only what the graph's outputs read, which is
//! what a device runs.
use std::collections::{HashMap, HashSet};

use arora_behavior_tree::{lower_behavior_tree, BehaviorTree, FunctionIndex, LoweredTree};
use arora_shape::{Shape, TypedPath, Value, WriteBatch, WriteOp};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::StructureField;
use uuid::Uuid;

use crate::eval::shape_helpers::infer_shape;
use crate::eval::{NodeFunctions, PortValue, SpawnRequest};
use crate::functions::{function_index, is_stateful, state_param, NodeGraphModule};
use crate::import::{import, ImportOptions, ImportedGraph};
use crate::types::{GraphSpec, NodeId, NodeType};

/// What an evaluation brings up to date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Evaluation {
    /// Every node, at every tick, as the reference evaluator evaluates them:
    /// each port can be read, and an error any node meets fails the tick.
    #[default]
    Every,
    /// What the graph's outputs read: its sinks, its keyed outputs and what
    /// they read. A node nothing reads does not run.
    Demanded,
}

/// The clock variables of the runtime's store, in seconds.
const TIME: &str = "graph/t";
const DT: &str = "graph/dt";

/// A node graph run on the behavior tree.
pub struct TreeRuntime {
    /// The graph's time, in seconds, as the next evaluation reads it.
    pub t: f32,
    /// The duration of the next evaluation's step, in seconds.
    pub dt: f32,
    /// Every observed node's ports after the last evaluation, by node id
    /// and port name.
    pub outputs: HashMap<NodeId, HashMap<String, PortValue>>,
    /// The writes the graph's `output` nodes made in the last evaluation, in
    /// evaluation order.
    pub writes: WriteBatch,
    /// The runs the graph's `spawn` nodes requested in the last evaluation.
    pub spawns: Vec<SpawnRequest>,
    evaluation: Evaluation,
    store: SimpleDataStore,
    module: NodeGraphModule,
    /// Values staged for the next evaluation, with their declared shapes.
    staged: HashMap<TypedPath, (Value, Option<Shape>)>,
    /// The input keys the last evaluation staged: unset at the next unless
    /// staged again.
    live: HashMap<Key, Option<Shape>>,
    loaded: Option<Loaded>,
}

/// A graph imported and lowered.
struct Loaded {
    /// The spec's JSON text, which an evaluation of a spec compares with.
    source: String,
    spec: GraphSpec,
    imported: ImportedGraph,
    lowered: LoweredTree,
    /// The graph's node indices in the reference evaluator's order.
    order: Vec<usize>,
}

impl Default for TreeRuntime {
    fn default() -> Self {
        Self::new(Evaluation::Every)
    }
}

impl TreeRuntime {
    /// A runtime holding no graph yet, evaluating as `evaluation` says.
    pub fn new(evaluation: Evaluation) -> Self {
        Self {
            t: 0.0,
            dt: 0.0,
            outputs: HashMap::new(),
            writes: WriteBatch::default(),
            spawns: Vec::new(),
            evaluation,
            store: SimpleDataStore::new(),
            module: NodeGraphModule::new(),
            staged: HashMap::new(),
            live: HashMap::new(),
            loaded: None,
        }
    }

    /// Route each external function of `function_modules` to its module.
    pub fn set_function_modules(&mut self, function_modules: HashMap<Uuid, Uuid>) {
        self.module.set_function_modules(function_modules);
    }

    /// Stage `value` on `path` for the next evaluation, which alone sees it.
    pub fn set_input(&mut self, path: TypedPath, value: Value, declared: Option<Shape>) {
        self.staged.insert(path, (value, declared));
    }

    /// The store the graph reads its inputs from and writes its outputs to.
    pub fn store(&self) -> &SimpleDataStore {
        &self.store
    }

    /// The tree the graph runs as, once loaded.
    pub fn tree(&self) -> Option<&BehaviorTree> {
        self.loaded.as_ref().map(|loaded| loaded.lowered.tree())
    }

    /// The graph as the tree holds it, once loaded.
    pub fn imported(&self) -> Option<&ImportedGraph> {
        self.loaded.as_ref().map(|loaded| &loaded.imported)
    }

    /// Run `spec` from now on. Every node of `spec` the previous graph also
    /// had keeps its state.
    pub fn load(&mut self, spec: &GraphSpec) -> Result<(), String> {
        self.load_with(spec, &mut NoModules)
    }

    fn load_with(&mut self, spec: &GraphSpec, inner: &mut dyn CallBridge) -> Result<(), String> {
        let options = ImportOptions {
            time: TIME.to_string(),
            dt: DT.to_string(),
        };
        let imported = import(spec, &options)?;
        let store = &self.store;
        let resolver = |name: &str| Some(store.slot(&Key::from(name)));
        let tree = imported.build(&resolver)?;
        let observed: Vec<Uuid> = match self.evaluation {
            Evaluation::Every => imported
                .graph_nodes
                .iter()
                .map(|node| node.tree_id)
                .collect(),
            Evaluation::Demanded => Vec::new(),
        };
        tree.observe_detached(
            observed
                .into_iter()
                .chain(imported.keyed_outputs.iter().map(|output| output.tree_id)),
        );
        let order = crate::topo::topo_order(&spec.nodes, &spec.edges)?;
        let index: HashMap<&str, usize> = spec
            .nodes
            .iter()
            .enumerate()
            .map(|(at, node)| (node.id.as_str(), at))
            .collect();
        let order = order.iter().map(|id| index[id.as_str()]).collect();
        let functions: FunctionIndex = function_index().clone();
        let mut bridge = self.module.bridge(inner);
        if let Some(previous) = &self.loaded {
            carry_state(&tree, previous, &imported);
        }
        let lowered =
            lower_behavior_tree(tree, &functions, &mut bridge, false).map_err(|e| e.to_string())?;
        if let Some(previous) = self.loaded.take() {
            previous.lowered.unregister(&mut bridge);
        }
        self.loaded = Some(Loaded {
            source: serde_json::to_string(spec).map_err(|e| e.to_string())?,
            spec: spec.clone(),
            imported,
            lowered,
            order,
        });
        Ok(())
    }

    /// Evaluate `spec`, loading it first when it is not the graph loaded.
    pub fn evaluate_spec(&mut self, spec: &GraphSpec) -> Result<(), String> {
        self.evaluate_spec_with(spec, &mut NoModules)
    }

    /// [`evaluate_spec`](Self::evaluate_spec), the calls the graph makes to
    /// other modules going to `inner`.
    pub fn evaluate_spec_with(
        &mut self,
        spec: &GraphSpec,
        inner: &mut dyn CallBridge,
    ) -> Result<(), String> {
        let source = serde_json::to_string(spec).map_err(|e| e.to_string())?;
        if self
            .loaded
            .as_ref()
            .is_none_or(|loaded| loaded.source != source)
        {
            self.load_with(spec, inner)?;
        }
        self.evaluate_with(inner)
    }

    /// Evaluate the loaded graph once.
    pub fn evaluate(&mut self) -> Result<(), String> {
        self.evaluate_with(&mut NoModules)
    }

    /// Evaluate the loaded graph once, the calls it makes to other modules
    /// going to `inner`.
    pub fn evaluate_with(&mut self, inner: &mut dyn CallBridge) -> Result<(), String> {
        self.outputs.clear();
        self.writes = WriteBatch::default();
        self.spawns.clear();
        let Some(loaded) = &self.loaded else {
            return Err("no graph is loaded".to_string());
        };

        // The clock, and the inputs staged for this evaluation alone.
        let mut change = StateChange::new();
        change.set.insert(Key::from(TIME), Some(Value::F32(self.t)));
        change.set.insert(Key::from(DT), Some(Value::F32(self.dt)));
        let staged = std::mem::take(&mut self.staged);
        let mut live = HashMap::new();
        for (path, (value, declared)) in staged {
            let key = Key::new(path.to_string());
            change.set.insert(key.clone(), Some(value));
            live.insert(key, declared);
        }
        for key in self.live.keys() {
            change.set.entry(key.clone()).or_insert(None);
        }
        self.live = live;
        self.store.write(change).map_err(|e| e.to_string())?;

        let mut bridge = self.module.bridge(inner);
        loaded
            .lowered
            .tick(&mut bridge)
            .map_err(|e| e.to_string())?;
        self.spawns = self.module.take_spawns();

        // What every observed node computed, and what the outputs wrote.
        let tree = loaded.lowered.tree();
        let mut keyed = StateChange::new();
        for at in &loaded.order {
            let (node, graph_node) = (&loaded.spec.nodes[*at], &loaded.imported.graph_nodes[*at]);
            let Some(returned) = loaded.imported.returned(tree, graph_node) else {
                continue;
            };
            let mut ports = HashMap::new();
            for port in &graph_node.ports {
                let Some(value) = graph_node.port_value(&returned, port) else {
                    continue;
                };
                let shape = node
                    .output_shapes
                    .get(port)
                    .cloned()
                    .or_else(|| {
                        let path = node.params.path.as_ref()?;
                        (node.kind == NodeType::Input)
                            .then(|| self.live.get(&Key::new(path.to_string())).cloned())??
                    })
                    .unwrap_or_else(|| infer_shape(&value));
                ports.insert(port.clone(), PortValue::with_shape(value, shape));
            }
            if node.kind == NodeType::Output {
                if let (Some(path), Some(port)) = (&node.params.path, ports.get("out")) {
                    self.writes.push(WriteOp::new_with_shape(
                        path.clone(),
                        port.value.clone(),
                        Some(port.shape.clone()),
                    ));
                } else if let (Some(key_field), Some(value_field), Some(port)) = (
                    node.params.key_field,
                    node.params.value_field,
                    ports.get("out"),
                ) {
                    for op in crate::eval::eval_node::keyed_batch_writes(
                        &port.value,
                        key_field,
                        value_field,
                    )
                    .map_err(|message| format!("node '{}': {message}", node.id))?
                    {
                        keyed
                            .set
                            .insert(Key::new(op.path.to_string()), Some(op.value.clone()));
                        self.writes.push(op);
                    }
                }
            }
            self.outputs.insert(node.id.clone(), ports);
        }
        if !keyed.set.is_empty() {
            self.store.write(keyed).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Carry into `tree`, built from `imported`, the state of `previous`: every
/// node `imported` shares unchanged with it keeps the cells its arguments
/// hold, and a changed stateful node keeps its state, as a node the reference
/// evaluator keeps runs on with its state when its parameters change.
fn carry_state(tree: &BehaviorTree, previous: &Loaded, imported: &ImportedGraph) {
    let held: HashMap<Uuid, &arora_behavior_tree::schema::Node> = previous
        .imported
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect();
    let fresh: HashSet<Uuid> = imported
        .nodes
        .iter()
        .filter(|node| held.get(&node.id) != Some(node))
        .map(|node| node.id)
        .collect();
    let before = previous.lowered.tree();
    tree.keep_state(before, &fresh);
    for node in &imported.graph_nodes {
        if fresh.contains(&node.tree_id) && is_stateful(&node.kind) {
            let state = before.argument_value(node.tree_id, state_param());
            if state.is_some() {
                tree.set_argument_value(node.tree_id, state_param(), state);
            }
        }
    }
}

/// Evaluate `spec` on `rt`: the tree's counterpart of
/// [`evaluate_all`](crate::evaluate_all).
pub fn evaluate_all(rt: &mut TreeRuntime, spec: &GraphSpec) -> Result<(), String> {
    rt.evaluate_spec(spec)
}

/// Evaluate `spec` on `rt`, its external functions and task runs calling
/// `functions`: the tree's counterpart of
/// [`evaluate_all_with_functions`](crate::evaluate_all_with_functions).
pub fn evaluate_all_with_functions(
    rt: &mut TreeRuntime,
    spec: &GraphSpec,
    functions: &mut dyn NodeFunctions,
) -> Result<(), String> {
    rt.module.set_default_module(Some(FUNCTIONS_MODULE));
    let result = rt.evaluate_spec_with(spec, &mut FunctionsBridge { functions });
    rt.module.set_default_module(None);
    result
}

/// The module a [`FunctionsBridge`] answers for.
const FUNCTIONS_MODULE: Uuid = Uuid::from_u128(0x6e6f_6465_2d67_7261_7068_0000_0000_00ff);

/// A bridge calling a [`NodeFunctions`] host, its out-parameters the call's
/// mutated parameters.
pub struct FunctionsBridge<'a> {
    /// The host the calls go to.
    pub functions: &'a mut dyn NodeFunctions,
}

impl CallBridge for FunctionsBridge<'_> {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let args: Vec<(Uuid, Value)> = call
            .args
            .into_iter()
            .map(|field| (field.id, *field.value))
            .collect();
        let module = call.module_id.filter(|module| *module != FUNCTIONS_MODULE);
        let (ret, mutated) = self
            .functions
            .call_module_with_outputs(module, call.id, &args)
            .map_err(|message| CallError::Generic { message })?;
        Ok(CallResult {
            ret,
            mutated: mutated
                .into_iter()
                .map(|(id, value)| StructureField {
                    id,
                    value: Box::new(value),
                })
                .collect(),
        })
    }

    fn arora_register_callable(&mut self, _: std::rc::Rc<dyn Callable>) -> CallableId {
        unreachable!("the node-graph bridge registers the tree's callables")
    }

    fn arora_unregister_callable(&mut self, _: &CallableId) {}

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        Err(CallError::Internal {
            message: format!("no callable {}", callable_id.id),
        })
    }
}

/// A bridge to no module: what a graph runs over when it calls none.
struct NoModules;

impl CallBridge for NoModules {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        Err(CallError::FunctionNotFound { id: call.id })
    }

    fn arora_register_callable(&mut self, _: std::rc::Rc<dyn Callable>) -> CallableId {
        unreachable!("the node-graph bridge registers the tree's callables")
    }

    fn arora_unregister_callable(&mut self, _: &CallableId) {}

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        Err(CallError::Internal {
            message: format!("no callable {}", callable_id.id),
        })
    }
}
