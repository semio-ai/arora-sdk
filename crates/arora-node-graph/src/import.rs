//! The importer: a Vizij node graph ([`GraphSpec`], its JSON form) as the
//! nodes of a behavior tree, run by `arora-behavior-tree`.
//!
//! The graph translates node for node:
//!
//! - **A node** is a call to its kind's function ([`crate::functions`]),
//!   which the tree holds **detached**: it runs when what it outputs is read,
//!   once per tick, and only when its arguments changed. Its node parameters
//!   are literal arguments; a stateful node's state is its literal `state`
//!   argument, which the call writes back.
//! - **An edge** is an argument reading the source node's output: `@node`
//!   for a node whose one port is `out`, a selection of the port's field out
//!   of the structure a multi-port node returns otherwise. An edge's selector
//!   is a [`select`](crate::functions::SELECT) node between the two.
//! - **An inline default** is a literal argument, for an input no edge feeds
//!   or one whose edge names a port the source does not have.
//! - **An `input` node** reads the store key its path names, a variable of
//!   that name. A node reading the clock reads the variables
//!   [`ImportOptions::time`] and [`ImportOptions::dt`].
//! - **An `output` node with a path** binds its return value to the store key
//!   of its path: it is a **sink**, written at every tick after the tree
//!   ticks; of two sinks of one key, the later wins, as the later `output`
//!   does in the graph. An `output` with no path, which writes the keys a
//!   batch of records names, is listed in [`ImportedGraph::keyed_outputs`]
//!   for its host to apply.
//! - **A node that acts** at each evaluation (`externalfunction`, `taskrun`,
//!   `spawn`) is a leaf the tree holds under its root `PARALLEL`, in the
//!   graph's order, ticked at every tick. Its return value is the tree-local
//!   variable `.node/<id>`, which its readers read. A graph with no such node
//!   has a `SUCCEED` leaf as its root.
use std::collections::{HashMap, HashSet};

use arora_behavior_tree::nodes::{PARALLEL_FUNCTION_ID, SUCCEED_FUNCTION_ID};
use arora_behavior_tree::schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID};
use arora_behavior_tree::variable::VariableResolver;
use arora_behavior_tree::{load_behavior_tree_nodes_with, BehaviorTree};
use arora_shape::Value;
use arora_types::data::Key;
use uuid::Uuid;

use crate::eval::plan::output_layout;
use crate::functions::{
    declared_inputs, dt_param, encode_params, encode_selector, external_function, function_id,
    input_param, is_action, is_stateful, node_param, output_field, output_shapes_param,
    ports_param, reads_dt, reads_time, select_function, selector_param, state_param, time_param,
    VARIADIC_PORTS,
};
use crate::types::{GraphSpec, InputConnection, NodeSpec, NodeType};

/// Where a translated graph reads its clock from.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// The variable the `time` and `oscillator` nodes read the graph's time
    /// from: seconds as a float, or nanoseconds as an unsigned integer.
    pub time: String,
    /// The variable the smoothing nodes read the step's duration from, in
    /// the same units.
    pub dt: String,
}

impl Default for ImportOptions {
    /// The device's clock: `arora/time` and `arora/dt`.
    fn default() -> Self {
        Self {
            time: arora_behavior::built_in::TIME.to_string(),
            dt: arora_behavior::built_in::DT.to_string(),
        }
    }
}

/// A node of the graph as the tree holds it.
#[derive(Clone, Debug)]
pub struct GraphNode {
    /// Its id in the graph.
    pub id: String,
    /// Its kind.
    pub kind: NodeType,
    /// The tree node that calls its function.
    pub tree_id: Uuid,
    /// Its output ports, in order. A node whose ports are `out` alone
    /// returns that port's value; any other returns a structure of them.
    pub ports: Vec<String>,
    /// Whether the tree holds it, as a leaf ticked at every tick, rather
    /// than detached.
    pub attached: bool,
}

impl GraphNode {
    /// Whether the node returns the value of its one port, `out`.
    pub fn returns_out(&self) -> bool {
        self.ports.len() == 1 && self.ports[0] == "out"
    }

    /// The value of its port `port` in `returned`, what its call returned:
    /// `None` for a port it does not have, or one its evaluation left unset.
    pub fn port_value(&self, returned: &Value, port: &str) -> Option<Value> {
        if self.returns_out() {
            return (port == "out" && *returned != Value::Unit).then(|| returned.clone());
        }
        let Value::Structure(structure) = returned else {
            return None;
        };
        let field = output_field(port);
        structure
            .fields
            .iter()
            .find(|candidate| candidate.id == field)
            .map(|field| (*field.value).clone())
            .filter(|value| *value != Value::Unit)
    }
}

/// An `output` node with no path, which writes each record of the batch it
/// receives under the key the record names.
#[derive(Clone, Debug)]
pub struct KeyedOutput {
    /// The node's id in the graph.
    pub id: String,
    /// The tree node that calls its function.
    pub tree_id: Uuid,
    /// The record field holding each entry's key.
    pub key_field: Uuid,
    /// The record field holding each entry's value.
    pub value_field: Uuid,
}

/// A graph translated to the nodes of a behavior tree.
#[derive(Clone, Debug)]
pub struct ImportedGraph {
    /// The tree's nodes, its root first.
    pub nodes: Vec<Node>,
    /// The name of every variable the nodes read or write, by id: a store
    /// key, or a tree-local name starting with `.`.
    pub names: HashMap<Uuid, String>,
    /// The graph's nodes, in the graph's order.
    pub graph_nodes: Vec<GraphNode>,
    /// The `output` nodes with no path, in the graph's order.
    pub keyed_outputs: Vec<KeyedOutput>,
}

/// The namespace of the tree node ids a graph's node ids derive from.
const NODES: Uuid = Uuid::from_u128(0x6e6f_6465_2d67_7261_7068_0000_0000_0002);
/// The namespace of variable ids, derived from their names.
const VARIABLES: Uuid = Uuid::from_u128(0x6e6f_6465_2d67_7261_7068_0000_0000_0003);

/// The tree node id of the graph's node `id`.
pub fn tree_node_id(id: &str) -> Uuid {
    Uuid::new_v5(&NODES, id.as_bytes())
}

/// The id of the variable named `name`.
pub fn variable_id(name: &str) -> Uuid {
    Uuid::new_v5(&VARIABLES, name.as_bytes())
}

/// The tree-local variable an attached node's return value is bound to.
fn attached_output(id: &str) -> String {
    format!(".node/{id}")
}

impl ImportedGraph {
    /// Build the behavior tree, binding each variable `resolver` resolves to
    /// its store slot and every other to a tree-local cell.
    pub fn build(&self, resolver: &VariableResolver) -> Result<BehaviorTree, String> {
        load_behavior_tree_nodes_with(self.nodes.clone(), resolver, &self.names)
            .map_err(|e| e.to_string())
    }

    /// The graph's node `id`.
    pub fn node(&self, id: &str) -> Option<&GraphNode> {
        self.graph_nodes.iter().find(|node| node.id == id)
    }

    /// What `tree` holds as the return value of the graph's node `node`:
    /// the cache of a detached node's last call, the variable an attached
    /// node writes.
    pub fn returned(&self, tree: &BehaviorTree, node: &GraphNode) -> Option<Value> {
        if node.attached {
            tree.argument_value(node.tree_id, _RET_PARAM_ID)
        } else {
            tree.detached_outputs(node.tree_id)?.remove(&_RET_PARAM_ID)
        }
    }
}

/// Translate a node graph from its JSON form: a `GraphSpec`, or a document
/// holding one under `spec`, in any form
/// [`normalize_graph_spec_value`](arora_shape::json::normalize_graph_spec_value)
/// accepts.
pub fn import_json(
    json: &str,
    options: &ImportOptions,
) -> Result<(GraphSpec, ImportedGraph), String> {
    let mut document: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("node graph JSON: {e}"))?;
    if let Some(spec) = document
        .get("spec")
        .filter(|spec| spec.get("nodes").is_some())
    {
        document = spec.clone();
    }
    arora_shape::json::normalize_graph_spec_value(&mut document)
        .map_err(|e| format!("node graph JSON: {e:?}"))?;
    let spec: GraphSpec =
        serde_json::from_value(document).map_err(|e| format!("node graph JSON: {e}"))?;
    let imported = import(&spec, options)?;
    Ok((spec, imported))
}

/// Translate `spec`.
///
/// Refuses, naming the node: an edge naming a node the graph does not have,
/// two edges into one input, two nodes of one id, and a variadic port past
/// the last a function declares
/// ([`VARIADIC_PORTS`](crate::functions::VARIADIC_PORTS)). A cycle is
/// refused when the tree is lowered.
pub fn import(spec: &GraphSpec, options: &ImportOptions) -> Result<ImportedGraph, String> {
    let connections = spec.input_connections()?;
    let mut seen = HashSet::new();
    for node in &spec.nodes {
        if !seen.insert(node.id.as_str()) {
            return Err(format!("two nodes share the id '{}'", node.id));
        }
    }

    // The ports each node's edges read, and so its output layout.
    let mut referenced: HashMap<&str, hashbrown::HashSet<String>> = HashMap::new();
    for edge in &spec.edges {
        referenced
            .entry(edge.from.node_id.as_str())
            .or_default()
            .insert(edge.from.output.clone());
    }
    let empty = hashbrown::HashSet::new();
    let graph_nodes: Vec<GraphNode> = spec
        .nodes
        .iter()
        .map(|node| {
            let layout = output_layout(node, referenced.get(node.id.as_str()).unwrap_or(&empty));
            GraphNode {
                id: node.id.clone(),
                kind: node.kind.clone(),
                tree_id: tree_node_id(&node.id),
                ports: layout.slots.clone(),
                attached: is_action(&node.kind),
            }
        })
        .collect();
    let by_id: HashMap<&str, &GraphNode> = graph_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    let mut names = HashMap::new();
    let mut variable = |name: &str| {
        let id = variable_id(name);
        names.insert(id, name.to_string());
        Expression::VariableId(id)
    };

    let mut tree_nodes = Vec::with_capacity(spec.nodes.len() + 1);
    let mut selects = Vec::new();
    let mut keyed_outputs = Vec::new();
    let no_connections = hashbrown::HashMap::new();
    for (node, graph_node) in spec.nodes.iter().zip(&graph_nodes) {
        let mut arguments: HashMap<Uuid, Expression> = HashMap::new();
        let declared: HashSet<String> = declared_inputs(&node.kind).into_iter().collect();
        let wired = connections.get(&node.id).unwrap_or(&no_connections);
        let mut ports: Vec<(&String, &InputConnection)> = wired.iter().collect();
        ports.sort_by(|a, b| a.0.cmp(b.0));
        for (port, connection) in ports {
            if !declared.contains(port.as_str()) {
                if is_variadic_past_the_last(&node.kind, port) {
                    return Err(format!(
                        "node '{}' wires its input '{port}', past the {VARIADIC_PORTS} ports \
                         of its variadic group a function declares",
                        node.id
                    ));
                }
                // An input the node's evaluation never reads.
                continue;
            }
            let read = connection.node_id.as_ref().and_then(|source| {
                let source = by_id.get(source.as_str())?;
                source
                    .ports
                    .contains(&connection.output_key)
                    .then(|| port_read(source, &connection.output_key, &mut variable))
            });
            let argument = match (read, &connection.default_value) {
                (Some(read), _) => match &connection.selector {
                    Some(selector) if !selector.is_empty() => {
                        let select_id =
                            Uuid::new_v5(&NODES, format!("select:{}:{port}", node.id).as_bytes());
                        selects.push(Node {
                            id: select_id,
                            function: select_function(),
                            arguments: HashMap::from([
                                (input_param("in"), read),
                                (
                                    selector_param(),
                                    Expression::Value(encode_selector(selector)),
                                ),
                                (
                                    node_param(),
                                    Expression::Value(Value::String(node.id.clone())),
                                ),
                            ]),
                            children: None,
                        });
                        Expression::NodeArgument(NodeParameterId {
                            node: select_id,
                            parameter: _RET_PARAM_ID,
                        })
                    }
                    _ => read,
                },
                (None, Some(default)) => Expression::Value(default.clone()),
                (None, None) => continue,
            };
            arguments.insert(input_param(port), argument);
        }

        for (parameter, value) in encode_params(&node.params)? {
            arguments.insert(parameter, Expression::Value(value));
        }
        arguments.insert(
            node_param(),
            Expression::Value(Value::String(node.id.clone())),
        );
        if !node.output_shapes.is_empty() {
            let shapes = serde_json::to_string(&node.output_shapes).map_err(|e| e.to_string())?;
            arguments.insert(
                output_shapes_param(),
                Expression::Value(Value::String(shapes)),
            );
        }
        if !graph_node.returns_out() {
            arguments.insert(
                ports_param(),
                Expression::Value(Value::ArrayString(graph_node.ports.clone())),
            );
        }
        if reads_time(&node.kind) {
            arguments.insert(time_param(), variable(&options.time));
        }
        if reads_dt(&node.kind) {
            arguments.insert(dt_param(), variable(&options.dt));
        }
        if is_stateful(&node.kind) {
            arguments.insert(state_param(), Expression::Value(Value::Unit));
        }
        if node.kind == NodeType::Input {
            if let Some(path) = &node.params.path {
                arguments.insert(
                    input_param("in"),
                    variable(&Key::new(path.to_string()).path),
                );
            }
        }
        if node.kind == NodeType::Output {
            match (
                &node.params.path,
                node.params.key_field,
                node.params.value_field,
            ) {
                (Some(path), ..) => {
                    arguments.insert(_RET_PARAM_ID, variable(&path.to_string()));
                }
                (None, Some(key_field), Some(value_field)) => keyed_outputs.push(KeyedOutput {
                    id: node.id.clone(),
                    tree_id: graph_node.tree_id,
                    key_field,
                    value_field,
                }),
                _ => {}
            }
        }
        if graph_node.attached {
            arguments.insert(_RET_PARAM_ID, variable(&attached_output(&node.id)));
        }
        tree_nodes.push(Node {
            id: graph_node.tree_id,
            function: function_of(node),
            arguments,
            children: None,
        });
    }

    let leaves: Vec<Uuid> = graph_nodes
        .iter()
        .filter(|node| node.attached)
        .map(|node| node.tree_id)
        .collect();
    let root = if leaves.is_empty() {
        Node {
            id: Uuid::new_v5(&NODES, b"root"),
            function: SUCCEED_FUNCTION_ID,
            arguments: HashMap::new(),
            children: None,
        }
    } else {
        Node {
            id: Uuid::new_v5(&NODES, b"root"),
            function: PARALLEL_FUNCTION_ID,
            arguments: HashMap::new(),
            children: Some(leaves),
        }
    };
    let nodes = std::iter::once(root)
        .chain(tree_nodes)
        .chain(selects)
        .collect();
    Ok(ImportedGraph {
        nodes,
        names,
        graph_nodes,
        keyed_outputs,
    })
}

/// The function a node of the graph calls.
fn function_of(node: &NodeSpec) -> Uuid {
    match node.kind {
        NodeType::ExternalFunction => external_function(),
        ref kind => function_id(kind),
    }
}

/// How a node reads the port `port` of `source`.
fn port_read(
    source: &GraphNode,
    port: &str,
    variable: &mut impl FnMut(&str) -> Expression,
) -> Expression {
    let returned = if source.attached {
        variable(&attached_output(&source.id))
    } else {
        Expression::NodeArgument(NodeParameterId {
            node: source.tree_id,
            parameter: _RET_PARAM_ID,
        })
    };
    if source.returns_out() {
        returned
    } else {
        Expression::Select {
            source: Box::new(returned),
            path: Key::new(format!(".{}", output_field(port))),
        }
    }
}

/// Whether `port` is a port of `kind`'s variadic group past the last its
/// function declares.
fn is_variadic_past_the_last(kind: &NodeType, port: &str) -> bool {
    let (group, index) = crate::eval::variadic::parse_variadic_key(port);
    let declared_group = crate::eval::plan::signatures()
        .get(kind)
        .and_then(|signature| signature.variadic_inputs.as_ref())
        .map(|variadic| variadic.id);
    declared_group == Some(group) && index.is_some_and(|index| index >= VARIADIC_PORTS)
}
