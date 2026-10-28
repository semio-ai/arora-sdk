//! Workload envelope around the behavior text language.
use crate::{forms::Nodes, host::Bridge};
use arora_behavior_tree::schema::{Expression, _RET_PARAM_ID};
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

impl Workload {
    pub fn parse(source: &str) -> Result<Self, String> {
        let raw: RawWorkload = serde_json::from_str(source).map_err(|e| e.to_string())?;
        let keys: Vec<_> = raw
            .keys
            .into_iter()
            .map(|k| (k.name, k.initial.map(Scalar::value)))
            .collect();
        if raw.warm_up >= raw.ticks || !raw.dt.is_finite() || raw.dt <= 0.0 {
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
        let attached = loaded
            .nodes
            .iter()
            .filter(|n| n.attached)
            .map(|n| n.id)
            .collect::<HashSet<_>>();
        // A direct duplicate binding is a sanity check. Aliases and ordering
        // are resolved by the runtime, not predicted by the workload model.
        for node in &nodes {
            let mut outputs = HashSet::new();
            for (id, expr) in &node.arguments {
                if is_output(functions, &node.function, id) {
                    if let Expression::VariableId(cell) = expr {
                        if !outputs.insert(cell) {
                            return Err(
                                "a call binds each key or cell to at most one output".into()
                            );
                        }
                    }
                }
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
