//! Workload envelope around the behavior text language.
use crate::{forms::Nodes, host::Bridge};
use arora_behavior_tree::schema::{Expression, Node, _RET_PARAM_ID};
use arora_behavior_tree::{
    graph::graph_to_bt_nodes, load_behavior_tree_text, text, variable::is_tree_local,
};
use arora_types::value::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone)]
pub struct Workload {
    pub keys: Vec<(String, Option<Value>)>,
    pub nodes: Nodes,
    pub attached: HashSet<uuid::Uuid>,
    pub compared: Vec<usize>,
    pub writer_keys: HashSet<usize>,
    pub dynamic_keys: bool,
    pub clock: usize,
    pub dt: f64,
    pub warm_up: usize,
    pub events: Vec<Vec<(usize, Value)>>,
    pub phase_names: Vec<String>,
    pub phases: Vec<usize>,
}
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Scalar {
    Number(f64),
    Boolean(bool),
    String(String),
}
impl Scalar {
    fn value(self) -> Value {
        match self {
            Self::Number(x) => Value::F64(x),
            Self::Boolean(x) => Value::Boolean(x),
            Self::String(x) => Value::String(x),
        }
    }
}
#[derive(serde::Deserialize)]
struct RawKey {
    name: String,
    initial: Option<Scalar>,
}
#[derive(serde::Deserialize)]
struct RawPhases {
    names: Vec<String>,
    runs: Vec<(usize, usize)>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWorkload {
    keys: Vec<RawKey>,
    text: String,
    clock: String,
    dt: f64,
    ticks: usize,
    warm_up: usize,
    events: Vec<(usize, String, Scalar)>,
    phases: RawPhases,
}
/// Whether a call's parameter is one it writes: its return or a mutable
/// parameter of its function or native.
pub(crate) fn is_output(
    functions: &arora_behavior::FunctionIndex,
    function: &Uuid,
    parameter: &Uuid,
) -> bool {
    *parameter == _RET_PARAM_ID
        || functions
            .get(function)
            .and_then(|f| f.function.parameters.get(parameter))
            .is_some_and(|p| p.mutable)
        || text::natives()
            .find(|f| f.function_id == *function)
            .and_then(|f| f.function.parameters.get(parameter))
            .is_some_and(|p| p.mutable)
}

/// The cell an output binding writes: a variable, or the cell another node's
/// input it is bound to reads.
pub(crate) fn written_cell(nodes: &HashMap<Uuid, &Node>, expr: &Expression) -> Option<Uuid> {
    let mut expr = expr;
    for _ in 0..=nodes.len() {
        match expr {
            Expression::VariableId(cell) => return Some(*cell),
            Expression::NodeArgument(port) => {
                expr = nodes.get(&port.node)?.arguments.get(&port.parameter)?;
            }
            _ => return None,
        }
    }
    None
}

/// The cells an input expression reads by name.
fn read_cells(expr: &Expression, cells: &mut Vec<Uuid>) {
    match expr {
        Expression::VariableId(cell) => cells.push(*cell),
        Expression::Select { source, .. } => read_cells(source, cells),
        Expression::Call(call) => {
            read_cells(&call.module, cells);
            read_cells(&call.function, cells);
            for (parameter, argument) in &call.arguments {
                read_cells(parameter, cells);
                read_cells(argument, cells);
            }
        }
        _ => {}
    }
}

impl Workload {
    pub fn parse(source: &str) -> Result<Self, String> {
        let raw: RawWorkload = serde_json::from_str(source).map_err(|e| e.to_string())?;
        let mut keys: Vec<_> = raw
            .keys
            .into_iter()
            .map(|k| (k.name, k.initial.map(Scalar::value)))
            .collect();
        if raw.ticks == 0 || raw.warm_up >= raw.ticks || !raw.dt.is_finite() || raw.dt <= 0.0 {
            return Err("invalid ticks, warmUp or dt".into());
        }
        let by_name: HashMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.clone(), i))
            .collect();
        if by_name.len() != keys.len() || keys.iter().any(|(name, _)| name.is_empty()) {
            return Err("key names must be nonempty and unique".into());
        }
        if keys.iter().any(|(name, _)| is_tree_local(name)) {
            return Err("key names must name store keys, not tree-local variables".into());
        }
        let clock = *by_name
            .get(raw.clock.as_str())
            .ok_or("clock names an unknown key")?;
        let functions = Bridge::index();
        // The loader owns syntax, resolution, types and diagnostics.
        load_behavior_tree_text(&raw.text, functions, &|_| None).map_err(|e| e.to_string())?;
        let loaded = text::resolve(&text::parse(&raw.text).document, functions);
        let nodes = graph_to_bt_nodes(&loaded.graph).map_err(|e| e.to_string())?;
        let dynamic_keys = nodes
            .iter()
            .any(|n| n.function == arora_behavior_tree::nodes::SCATTER_FUNCTION_ID);
        let attached = loaded
            .nodes
            .iter()
            .filter(|n| n.attached)
            .map(|n| n.id)
            .collect::<HashSet<_>>();
        let by_id: HashMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
        let name_of = |cell: &Uuid| {
            loaded
                .graph
                .variables
                .get(cell)
                .cloned()
                .unwrap_or_else(|| cell.to_string())
        };
        let mut written_names = HashSet::new();
        // Per cell, a store key or a tree-local variable: the graph calls and
        // the tree nodes that write it.
        let mut writers: HashMap<Uuid, (Vec<Uuid>, usize)> = HashMap::new();
        for node in &nodes {
            let graph = !attached.contains(&node.id);
            let mut outputs = HashSet::new();
            for (id, expr) in &node.arguments {
                if !is_output(functions, &node.function, id) {
                    continue;
                }
                // Attached-naive orders graph calls by the ports they read, so
                // a graph call writing another node's input would reorder its
                // reader unseen.
                if graph && matches!(expr, Expression::NodeArgument(_)) {
                    return Err(
                        "a graph call's output binds a key or a cell, not another node's input"
                            .into(),
                    );
                }
                let Some(cell) = written_cell(&by_id, expr) else {
                    continue;
                };
                if !outputs.insert(cell) {
                    return Err("a call binds each key or cell to at most one output".into());
                }
                if let Some(name) = loaded
                    .graph
                    .variables
                    .get(&cell)
                    .filter(|n| !is_tree_local(n))
                {
                    written_names.insert(name.clone());
                }
            }
            for cell in outputs {
                let entry = writers.entry(cell).or_default();
                if graph {
                    entry.0.push(node.id);
                } else {
                    entry.1 += 1;
                }
            }
        }
        // Detached writes graph calls' cells after the tree, and evaluates
        // graph calls on demand within it; attached-naive runs graph calls in
        // its own order around the tree. A second writer of a cell a graph
        // call writes would leave whichever write the form orders last, and a
        // reader of that cell would see a different tick's value in each
        // form, so neither can be compared. A graph call's value is read
        // through its port.
        if let Some((cell, _)) = writers
            .iter()
            .find(|(_, (graph, tree))| !graph.is_empty() && graph.len() + tree > 1)
        {
            return Err(format!(
                "a key a graph call writes has no other writer: {}",
                name_of(cell)
            ));
        }
        for node in &nodes {
            let mut read = Vec::new();
            for (id, expr) in &node.arguments {
                if !is_output(functions, &node.function, id) {
                    read_cells(expr, &mut read);
                }
            }
            if let Some(cell) = read.iter().find(|cell| {
                writers
                    .get(cell)
                    .is_some_and(|(graph, _)| graph.iter().any(|w| *w != node.id))
            }) {
                return Err(format!(
                    "a graph call's value is read through its port, not through {}",
                    name_of(cell)
                ));
            }
        }
        let mut events = vec![Vec::new(); raw.ticks];
        for (tick, name, value) in raw.events {
            let key = *by_name
                .get(name.as_str())
                .ok_or("event names an unknown key")?;
            if tick >= raw.ticks {
                return Err("event tick out of range".into());
            }
            events[tick].push((key, value.value()));
        }
        // Text can introduce store keys without an initial envelope value.
        let mut introduced: Vec<_> = written_names
            .iter()
            .filter(|name| !by_name.contains_key(*name))
            .cloned()
            .collect();
        introduced.sort();
        keys.extend(introduced.into_iter().map(|name| (name, None)));
        let compared: Vec<_> = keys
            .iter()
            .enumerate()
            .filter_map(|(k, (name, _))| written_names.contains(name).then_some(k))
            .collect();
        let written = if dynamic_keys {
            (0..keys.len()).collect()
        } else {
            compared.iter().copied().collect()
        };
        if raw.phases.names.is_empty()
            || raw
                .phases
                .names
                .iter()
                .any(|n| n.is_empty() || n.contains([',', ';']))
        {
            return Err("invalid phase names".into());
        }
        let mut phases = vec![0; raw.ticks];
        let mut last = 0;
        for (start, phase) in raw.phases.runs {
            if start >= raw.ticks || start < last || phase >= raw.phases.names.len() {
                return Err("invalid phase run".into());
            }
            phases[start..].fill(phase);
            last = start;
        }
        Ok(Self {
            keys,
            nodes: (nodes, loaded.graph.variables),
            attached,
            compared,
            writer_keys: written,
            dynamic_keys,
            clock,
            dt: raw.dt,
            warm_up: raw.warm_up,
            events,
            phase_names: raw.phases.names,
            phases,
        })
    }
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        Self::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
}
