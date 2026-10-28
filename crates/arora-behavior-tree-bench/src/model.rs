//! Workload model. The crate README defines its JSON format.
use crate::host::Op;
use arora_behavior_tree::nodes::{FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID};
use arora_types::value::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
/// Where an argument's value comes from.
#[derive(Clone)]
pub enum Source {
    /// A store key, by its index in [`Workload::keys`].
    Key(usize),
    /// A graph call's return, by its index in [`Workload::calls`].
    Call(usize),
    Value(Value),
}
pub struct GraphCall {
    pub op: Op,
    pub arguments: Vec<(Uuid, Source)>,
    /// The key a sink publishes its return to.
    pub publishes: Option<usize>,
}
pub enum Tree {
    Composite(Uuid, Vec<Tree>),
    Equal(usize, Value),
    /// A module call returning a status: its call site, arguments, output
    /// keys and number of state parameters.
    Call(usize, Op, Vec<Source>, Vec<usize>, usize),
    Publish(usize, usize),
    Calls(Vec<usize>),
    Run,
}
pub struct Workload {
    /// Store keys and their initial values.
    pub keys: Vec<(String, Option<Value>)>,
    /// Every graph call, in data order: a call reads only calls before it.
    pub calls: Vec<GraphCall>,
    pub tree: Tree,
    /// The calls a `calls` marker places in the tree.
    pub marked: HashSet<usize>,
    /// The key each `publish` leaf writes its call's value to.
    pub published: HashMap<usize, usize>,
    /// The calls that read a key the tree writes, directly or through other
    /// calls, so they run after the tree.
    pub after: HashSet<usize>,
    /// Every key a sink or the tree writes, which the forms must agree on.
    pub compared: Vec<usize>,
    /// The keys the tree writes.
    pub tree_keys: HashSet<usize>,
    pub clock: usize,
    pub dt: f64,
    pub warm_up: usize,
    /// Each tick's key writes, before it.
    pub events: Vec<Vec<(usize, Value)>>,
    pub phase_names: Vec<String>,
    /// Each tick's phase, for its statistics and the series.
    pub phases: Vec<usize>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Scalar {
    Number(f64),
    Boolean(bool),
}
impl Scalar {
    fn value(self) -> Value {
        match self {
            Self::Number(x) => Value::F64(x),
            Self::Boolean(x) => Value::Boolean(x),
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum RawSource {
    Key { key: usize },
    Call { node: usize },
    Value { value: Scalar },
}
impl RawSource {
    fn resolve(self, keys: usize, calls: usize) -> Result<Source, String> {
        match self {
            Self::Key { key } if key < keys => Ok(Source::Key(key)),
            Self::Call { node } if node < calls => Ok(Source::Call(node)),
            Self::Value { value } => Ok(Source::Value(value.value())),
            _ => Err("source index out of range or graph not in data order".into()),
        }
    }
}
#[derive(serde::Deserialize)]
struct RawKey {
    name: String,
    initial: Option<Scalar>,
}
#[derive(serde::Deserialize)]
struct RawCall {
    op: String,
    args: Vec<(String, RawSource)>,
    publishes: Option<usize>,
}
#[derive(serde::Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
enum RawTree {
    Parallel {
        children: Vec<RawTree>,
    },
    Seq {
        children: Vec<RawTree>,
    },
    Fallback {
        children: Vec<RawTree>,
    },
    Equal {
        key: usize,
        value: Scalar,
    },
    Call {
        op: String,
        args: Vec<(String, RawSource)>,
        outputs: Vec<(String, usize)>,
        state: usize,
    },
    Publish {
        node: usize,
        key: usize,
    },
    Calls {
        calls: Vec<usize>,
    },
    Run,
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
    calls: Vec<RawCall>,
    tree: RawTree,
    clock: usize,
    dt: f64,
    ticks: usize,
    warm_up: usize,
    events: Vec<(usize, usize, Scalar)>,
    phases: RawPhases,
}

fn operation(
    name: &str,
    n: usize,
    outputs: usize,
    states: usize,
    action: bool,
) -> Result<Op, String> {
    let valid = match name {
        "sum" | "product" | "max" | "mix" => !action && n > 0,
        "first" | "publish" => !action && n == 1,
        "sub" | "div" | "noise" | "damp" | "chain-add" | "chain-mul" | "chain-clamp"
        | "chain-remap" => !action && n == 2,
        "clamp" | "osc" => !action && n == 3,
        "overlay" => !action && n == 4,
        "remap" => !action && n == 5,
        "above" => action && n == 2 && outputs == 0 && states == 0,
        "hold" => action && n == 2 + outputs && states == 2,
        "follow" | "set" => action && n == outputs && states == 0,
        "pulse" => action && n == 7 && outputs == 1 && states == 3,
        "wander" => action && n == 4 + 2 * outputs && states == 2,
        "keyframes" => {
            action
                && outputs > 0
                && n >= 3 + outputs
                && (n - 2).is_multiple_of(1 + outputs)
                && states == 3
        }
        _ => false,
    };
    if !valid {
        return Err(format!("invalid operation or signature: {name} ({n} arguments, {outputs} outputs, {states} states)"));
    }
    Ok(Op::parse(name))
}

struct Parser {
    marked: HashSet<usize>,
    published: HashMap<usize, usize>,
    written: HashSet<usize>,
    sites: usize,
    keys: usize,
    calls: usize,
}
impl Parser {
    fn key(&self, key: usize) -> Result<usize, String> {
        if key < self.keys {
            Ok(key)
        } else {
            Err("key index out of range".into())
        }
    }
    fn call(&self, call: usize) -> Result<usize, String> {
        if call < self.calls {
            Ok(call)
        } else {
            Err("call index out of range".into())
        }
    }
    fn node(&mut self, node: RawTree) -> Result<Tree, String> {
        match node {
            RawTree::Parallel { children }
            | RawTree::Seq { children }
            | RawTree::Fallback { children }
                if children.is_empty() =>
            {
                Err("empty composite".into())
            }
            RawTree::Parallel { children } => Ok(Tree::Composite(
                PARALLEL_FUNCTION_ID,
                self.children(children)?,
            )),
            RawTree::Seq { children } => {
                Ok(Tree::Composite(SEQ_FUNCTION_ID, self.children(children)?))
            }
            RawTree::Fallback { children } => Ok(Tree::Composite(
                FALLBACK_FUNCTION_ID,
                self.children(children)?,
            )),
            RawTree::Equal { key, value } => Ok(Tree::Equal(self.key(key)?, value.value())),
            RawTree::Call {
                op,
                args,
                outputs,
                state,
            } => {
                let op = operation(&op, args.len(), outputs.len(), state, true)?;
                let args = args
                    .into_iter()
                    .map(|(_, a)| a.resolve(self.keys, self.calls))
                    .collect::<Result<_, _>>()?;
                let outputs = outputs
                    .into_iter()
                    .map(|(_, k)| self.key(k))
                    .collect::<Result<Vec<_>, _>>()?;
                self.written.extend(&outputs);
                let site = self.sites;
                self.sites += 1;
                Ok(Tree::Call(site, op, args, outputs, state))
            }
            RawTree::Publish { node, key } => {
                let (node, key) = (self.call(node)?, self.key(key)?);
                self.published.insert(node, key);
                self.written.insert(key);
                Ok(Tree::Publish(node, key))
            }
            RawTree::Calls { calls } => {
                let calls = calls
                    .into_iter()
                    .map(|c| self.call(c))
                    .collect::<Result<Vec<_>, _>>()?;
                self.marked.extend(&calls);
                Ok(Tree::Calls(calls))
            }
            RawTree::Run => Ok(Tree::Run),
        }
    }
    fn children(&mut self, children: Vec<RawTree>) -> Result<Vec<Tree>, String> {
        children.into_iter().map(|node| self.node(node)).collect()
    }
}

impl Workload {
    /// Parse and validate indices, operation signatures and replay dimensions.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawWorkload = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let keys: Vec<_> = raw
            .keys
            .into_iter()
            .map(|k| (k.name, k.initial.map(Scalar::value)))
            .collect();
        if raw.ticks == 0
            || raw.warm_up >= raw.ticks
            || raw.clock >= keys.len()
            || !raw.dt.is_finite()
            || raw.dt <= 0.0
        {
            return Err("invalid ticks, warmUp, clock or dt".into());
        }
        let unique: HashSet<_> = keys.iter().map(|(name, _)| name).collect();
        if unique.len() != keys.len() || keys.iter().any(|(name, _)| name.is_empty()) {
            return Err("key names must be nonempty and unique".into());
        }
        let calls = raw
            .calls
            .into_iter()
            .enumerate()
            .map(|(c, call)| {
                let op = operation(&call.op, call.args.len(), 0, 0, false)?;
                let arguments = call
                    .args
                    .into_iter()
                    .enumerate()
                    .map(|(i, (_, a))| Ok((op.role(i), a.resolve(keys.len(), c)?)))
                    .collect::<Result<_, String>>()?;
                if call.publishes.is_some_and(|k| k >= keys.len()) {
                    return Err("sink key out of range".into());
                }
                Ok(GraphCall {
                    op,
                    arguments,
                    publishes: call.publishes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut parser = Parser {
            marked: HashSet::new(),
            published: HashMap::new(),
            written: HashSet::new(),
            sites: 0,
            keys: keys.len(),
            calls: calls.len(),
        };
        let tree = parser.node(raw.tree)?;
        if matches!(tree, Tree::Calls(_)) {
            return Err("root must be executable".into());
        }
        let mut after = HashSet::new();
        for (c, call) in calls.iter().enumerate() {
            if call.arguments.iter().any(|(_, s)| match s {
                Source::Key(k) => parser.written.contains(k),
                Source::Call(c) => after.contains(c),
                _ => false,
            }) {
                if parser.marked.contains(&c) {
                    return Err("a calls marker reads a tree output".into());
                }
                after.insert(c);
            }
        }
        let mut compared: Vec<_> = calls
            .iter()
            .filter_map(|c| c.publishes)
            .chain(parser.written.iter().copied())
            .collect();
        compared.sort_unstable();
        compared.dedup();
        let mut events = vec![Vec::new(); raw.ticks];
        for (tick, key, value) in raw.events {
            if tick >= raw.ticks || key >= keys.len() {
                return Err("event index out of range".into());
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
            calls,
            tree,
            marked: parser.marked,
            published: parser.published,
            after,
            compared,
            tree_keys: parser.written,
            clock: raw.clock,
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
