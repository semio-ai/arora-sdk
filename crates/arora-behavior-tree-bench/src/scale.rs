//! Deterministic scale scenarios, expressed in the common workload model.
use crate::{
    host::Op,
    model::{GraphCall as ModelCall, Source as ModelSource, Tree, Workload},
};
use arora_behavior_tree::nodes::{FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID};
use arora_types::value::Value;
use std::collections::{HashMap, HashSet};
const DT: f64 = 1.0 / 30.0;
/// Where an argument's value comes from.
#[derive(Clone, Copy)]
enum Source {
    /// A store key, by its index in [`Graph::keys`].
    Key(usize),
    /// Another call's return, by its index in [`Graph::calls`].
    Call(usize),
    Value(f64),
}
struct GraphCall {
    op: Op,
    arguments: Vec<Source>,
    /// The key the call publishes its return to.
    publishes: Option<usize>,
}
/// One graph: a branch of the tree, or the whole tree when it is the only one.
struct Body {
    calls: Vec<usize>,
    /// The boolean key that takes this branch; `None` for a lone graph.
    on: Option<usize>,
}
struct Graph {
    /// Store keys and their initial values.
    keys: Vec<(String, Option<Value>)>,
    /// The keys the scenarios change, in order.
    inputs: Vec<usize>,
    time: Option<usize>,
    /// Every call, in data order: a call reads only calls before it.
    calls: Vec<GraphCall>,
    bodies: Vec<Body>,
}
impl Graph {
    fn key(&mut self, name: String, initial: Option<Value>) -> usize {
        self.keys.push((name, initial));
        self.keys.len() - 1
    }
}

/// Independent arithmetic chains.
fn chains(n: usize) -> Graph {
    let k = 2.max(n / 10);
    let mut graph = Graph {
        keys: Vec::new(),
        inputs: Vec::new(),
        time: None,
        calls: Vec::new(),
        bodies: Vec::new(),
    };
    let inputs = (0..k)
        .map(|j| graph.key(format!("input/{j}"), Some(Value::F64(0.0))))
        .collect();
    graph.inputs = inputs;
    let outputs: Vec<_> = (0..k)
        .map(|j| graph.key(format!("output/{j}"), None))
        .collect();
    for i in 0..n {
        let x = if i < k {
            Source::Key(graph.inputs[i])
        } else {
            Source::Call(i - k)
        };
        graph.calls.push(GraphCall {
            op: [Op::ChainAdd, Op::ChainMul, Op::ChainClamp, Op::ChainRemap][(i / k) % 4],
            arguments: vec![x, Source::Value(1.1)],
            publishes: (i + k >= n).then(|| outputs[i % k]),
        });
    }
    graph.bodies.push(Body {
        calls: (0..n).collect(),
        on: None,
    });
    graph
}

/// A 32-bit linear congruential generator, scaled to [0, 1).
struct Random(u32);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0) / 4_294_967_296.0
    }
}

/// Deterministic random graphs for clock and branch scenarios.
fn generated(branches: bool, n: usize) -> Graph {
    const SEED: u32 = 332_396;
    let mut random = Random(SEED);
    let mut clock_random = Random(SEED ^ 0xc10c);
    let mut clock_calls = HashSet::new();
    if !branches {
        let mut ranks: Vec<_> = (0..n).map(|i| (i, clock_random.next())).collect();
        ranks.sort_by(|a, b| a.1.total_cmp(&b.1));
        let count = 1.max((n as f64 / 10.0).round() as usize);
        clock_calls.extend(ranks[..count].iter().map(|(i, _)| *i));
    }
    let mut graph = Graph {
        keys: Vec::new(),
        inputs: Vec::new(),
        time: None,
        calls: Vec::new(),
        bodies: Vec::new(),
    };
    let (parts, size) = if branches { (8, n.div_ceil(8)) } else { (1, n) };
    if !branches {
        graph.time = Some(graph.key("arora/time".into(), Some(Value::F64(0.0))));
    }
    for j in 0..parts {
        let (prefix, key_prefix) = if branches {
            (format!("b{j}_"), format!("branch/{j}/"))
        } else {
            (String::new(), "generated/".to_string())
        };
        let k = 2.max(size / 10);
        let inputs: Vec<_> = (0..k)
            .map(|i| graph.key(format!("{key_prefix}input/{i}"), Some(Value::F64(0.25))))
            .collect();
        graph.inputs.extend(&inputs);
        let first = graph.calls.len();
        let mut read = HashSet::new();
        for i in 0..size {
            let p = random.next();
            let x = if i < k {
                Source::Key(inputs[i])
            } else if random.next() < 0.8 {
                let back = (random.next() * i.min(32) as f64).floor() as usize;
                Source::Call(first + i - 1 - back)
            } else {
                Source::Key(inputs[(random.next() * k as f64).floor() as usize])
            };
            let value = Source::Value;
            let (mut op, mut arguments) = if p < 0.30 {
                (Op::Sum, vec![x, value(0.02)])
            } else if p < 0.60 {
                (Op::Product, vec![x, value(0.5 + random.next() * 0.3)])
            } else if p < 0.75 {
                (Op::Clamp, vec![x, value(-1.0), value(1.0)])
            } else if p < 0.95 {
                let remap = [x, value(-2.0), value(2.0)];
                let out = [value(-0.5), value(0.5)];
                (Op::Remap, remap.into_iter().chain(out).collect())
            } else {
                (Op::Damp, vec![x, value(0.06)])
            };
            if clock_calls.contains(&i) {
                let frequency = 0.2 + clock_random.next() * 0.8;
                op = Op::Osc;
                arguments = vec![Source::Key(graph.time.unwrap()), value(frequency)];
            }
            for source in &arguments {
                if let Source::Call(c) = source {
                    read.insert(*c);
                }
            }
            graph.calls.push(GraphCall {
                op,
                arguments,
                publishes: None,
            });
        }
        let calls: Vec<_> = (first..first + size).collect();
        let mut published = Vec::new();
        published.extend(calls.iter().filter(|c| !read.contains(c)));
        for c in &calls {
            if published.len() >= size.div_ceil(10) {
                break;
            }
            if !published.contains(c) {
                published.push(*c);
            }
        }
        for c in published {
            let name = format!("{key_prefix}output/{prefix}n{}", c - first);
            graph.calls[c].publishes = Some(graph.key(name, None));
        }
        let on =
            branches.then(|| graph.key(format!("{key_prefix}on"), Some(Value::Boolean(j == 0))));
        graph.bodies.push(Body { calls, on });
    }
    graph
}

#[derive(Clone, Copy, PartialEq)]
pub enum Scenario {
    Settled,
    OneInput,
    AllInputs,
    Clock,
    Branches,
    BranchesSwitching,
}
impl Scenario {
    pub const ALL: [Scenario; 6] = [
        Scenario::Settled,
        Scenario::OneInput,
        Scenario::AllInputs,
        Scenario::Clock,
        Scenario::Branches,
        Scenario::BranchesSwitching,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Settled => "settled",
            Scenario::OneInput => "one-input",
            Scenario::AllInputs => "all-inputs",
            Scenario::Clock => "clock",
            Scenario::Branches => "branches",
            Scenario::BranchesSwitching => "branches-switching",
        }
    }
    fn graph(self, n: usize) -> Graph {
        match self {
            Scenario::Settled | Scenario::OneInput | Scenario::AllInputs => chains(n),
            Scenario::Clock => generated(false, n),
            Scenario::Branches | Scenario::BranchesSwitching => generated(true, n),
        }
    }
    /// The key writes before tick `step`, the first being 0.
    fn changes(self, graph: &Graph, step: usize, mut write: impl FnMut(usize, Value)) {
        match self {
            Scenario::Settled => {}
            Scenario::OneInput => write(graph.inputs[0], Value::F64((step % 17) as f64)),
            Scenario::AllInputs => {
                for &key in &graph.inputs {
                    write(key, Value::F64((step % 17) as f64));
                }
            }
            Scenario::Clock => write(graph.time.unwrap(), Value::F64((step + 1) as f64 * DT)),
            Scenario::Branches | Scenario::BranchesSwitching => {
                for (i, &key) in graph.inputs.iter().enumerate() {
                    write(key, Value::F64(((step + i) % 17) as f64 / 8.0 - 1.0));
                }
                let taken = if self == Scenario::Branches {
                    0
                } else {
                    step / 50 % 8
                };
                for (j, body) in graph.bodies.iter().enumerate() {
                    write(body.on.unwrap(), Value::Boolean(j == taken));
                }
            }
        }
    }
}

pub const SIZES: &[usize] = &[10, 30, 100, 300, 1000];

impl Scenario {
    pub fn workload(self, n: usize, warm_up: usize, ticks: usize) -> Workload {
        assert!(n > 0 && ticks > 0);
        let mut graph = self.graph(n);
        let clock = graph
            .time
            .unwrap_or_else(|| graph.key("arora/time".into(), Some(Value::F64(0.0))));
        let mut events = Vec::new();
        for step in 0..warm_up + ticks {
            let mut changes = Vec::new();
            self.changes(&graph, step, |k, v| changes.push((k, v)));
            events.push(changes);
        }
        let branches = graph.bodies[0].on.is_some();
        let compared: Vec<_> = graph.calls.iter().filter_map(|c| c.publishes).collect();
        let source = |s: Source| match s {
            Source::Key(k) => ModelSource::Key(k),
            Source::Call(c) => ModelSource::Call(c),
            Source::Value(v) => ModelSource::Value(Value::F64(v)),
        };
        let calls = graph
            .calls
            .iter()
            .map(|c| {
                let mut op = c.op;
                let mut arguments: Vec<_> = c.arguments.iter().map(|s| source(*s)).collect();
                // The common oscillator takes frequency, phase and time.
                if op == Op::Osc {
                    arguments = vec![
                        arguments[1].clone(),
                        ModelSource::Value(Value::F64(0.0)),
                        arguments[0].clone(),
                    ];
                }
                // Continuous and paused damping have different semantics on reentry.
                // The switching scenario isolates branch scheduling with stateless calls.
                if self == Scenario::BranchesSwitching && op == Op::Damp {
                    op = Op::First;
                    arguments.truncate(1);
                }
                ModelCall {
                    op,
                    arguments: arguments
                        .into_iter()
                        .enumerate()
                        .map(|(i, s)| (op.role(i), s))
                        .collect(),
                    publishes: c.publishes.filter(|_| !branches),
                }
            })
            .collect();
        let mut published = HashMap::new();
        let tree = if !branches {
            Tree::Run
        } else {
            let children = graph
                .bodies
                .iter()
                .map(|body| {
                    let mut leaves: Vec<_> = body
                        .calls
                        .iter()
                        .filter_map(|&c| {
                            graph.calls[c].publishes.map(|k| {
                                published.insert(c, k);
                                Tree::Publish(c, k)
                            })
                        })
                        .collect();
                    leaves.push(Tree::Run);
                    Tree::Composite(
                        SEQ_FUNCTION_ID,
                        vec![
                            Tree::Equal(body.on.unwrap(), Value::Boolean(true)),
                            Tree::Composite(PARALLEL_FUNCTION_ID, leaves),
                        ],
                    )
                })
                .collect();
            Tree::Composite(FALLBACK_FUNCTION_ID, children)
        };
        let writer_keys = compared.iter().copied().collect();
        Workload {
            keys: graph.keys,
            calls,
            tree,
            marked: HashSet::new(),
            published,
            after: HashSet::new(),
            compared,
            writer_keys,
            clock,
            dt: DT,
            warm_up,
            phases: vec![0; warm_up + ticks],
            events,
            phase_names: vec![self.name().into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chain_operations_follow_depth_for_every_default_size() {
        for &n in SIZES {
            let graph = chains(n);
            let k = graph.inputs.len();
            for (i, call) in graph.calls.iter().enumerate() {
                assert_eq!(
                    call.op,
                    [Op::ChainAdd, Op::ChainMul, Op::ChainClamp, Op::ChainRemap][(i / k) % 4]
                );
            }
        }
    }
}
