//! `cargo bench -p arora-behavior-tree --bench scale -- [ticks] [samples] [N/scenario/form]`
//! prints one row per graph size, scenario and form, or only the row the
//! third argument names (`1000/clock/detached`, for a profiler): the median microseconds
//! per tick over the samples and their interquartile range, with module calls
//! and engine indirect dispatches per measured tick. Defaults: 200 warm-up
//! ticks, then 7 samples of 500 ticks. Each tick's input writes are timed with
//! it, and lowering is not.
//!
//! Graphs have N = 10, 30, 100, 300 and 1000 module calls. The scenarios:
//!
//! - `settled`, `one-input`, `all-inputs`: the `incremental` bench's graph. K
//!   = max(2, N/10) store inputs `input/j` feed K independent chains of add,
//!   mul, clamp and remap; call `i` reads input `i` when `i < K`, else the
//!   return of call `i - K`; the last call of each chain publishes to
//!   `output/j`. No input changes, input 0 changes, or every input changes,
//!   at every tick.
//! - `clock`: a generated graph of N calls, a tenth of them oscillators that
//!   read the store key `arora/time`, which advances by 1/30 at every tick.
//!   No other input changes.
//! - `branches`, `branches-switching`: eight generated graphs of N/8 calls
//!   (rounded up), one per branch of `FALLBACK[ SEQ[ Equal({branch/j/on},
//!   true), body_j ] × 8 ]`. Every input changes at every tick. The first
//!   branch is taken throughout, or the taken branch moves on every 50 ticks.
//!
//! A generated graph has K = max(2, n/10) inputs per graph, each starting at
//! 0.25. Call `i` reads input `i` when `i < K`; otherwise one of the previous 32
//! calls with probability 0.8, else a random input. It is an add (30%), mul
//! (30%), clamp (15%), remap (20%) or damp (5%), whose state is a literal on
//! its mutable `state` parameter. Every call that no call reads publishes, and
//! so do the first calls until n/10 (rounded up) publish. The generator and
//! its seed are those of the design comparison's JavaScript model, number for
//! number, so the two measure the same graphs.
//!
//! The forms. A lone graph's published call binds its `_ret` to its key, or is
//! a sink. A branch's graph is published by its branch, so only the taken
//! branch publishes, each form the way its code can: incremental leaves sit in
//! their branch and bind `_ret` to the key; placement's file publishes with the
//! native `Copy`; today's executor and detached nodes call `publish`, a module
//! function returning its argument with `_ret` bound to the key, since the
//! tree has no native node that writes a number to a key.
//!
//! - `attached-naive`: today's executor. Every call is a leaf of the root
//!   `PARALLEL`, in data order, ahead of the tree, and runs at every tick,
//!   whichever branch the tree takes. An attached reader of a call a lone
//!   graph publishes reads its key; of any other, the tree-local variable its
//!   `_ret` is bound to.
//! - `detached`: every call is a detached node, read as `@node`. A call a lone
//!   graph publishes is a sink; a branch's `publish` leaves read their calls,
//!   so a branch the tree does not take is not computed.
//! - `attached-incremental`: the attached leaves, `_on_change`, on the
//!   incremental executor, each graph's leaves in its own branch:
//!   `PARALLEL[ leaf × n, RUN ]`.
//! - `placed`: the detached form placed with `place_detached(…,
//!   PlacementScope::Readers)`, on the incremental executor. Sinks place before
//!   the root, and a branch's groups before the branch's body, which reads them.
//!
//! The bench compares the values the forms publish. In `branches-switching`, it
//! leaves today's executor out: its damping keeps running in a branch the tree
//! does not take, where the other forms' damping pauses.
//!
//! The host is a native test bridge, not wasm execution. It answers owned calls
//! (`arora_call`) and lent ones (`arora_call_borrowed`) alike, reading the
//! arguments where they are and recording none. Compare counts as well as
//! times.
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::hint::black_box;
use std::rc::Rc;
use std::time::Instant;

use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_behavior_tree::nodes::{
    COPY_FUNCTION_ID, COPY_OUT_PARAM_ID, COPY_VALUE_PARAM_ID, EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID,
    EQUAL_FUNCTION_ID, FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID,
    SEQ_FUNCTION_ID,
};
use arora_behavior_tree::schema::{
    Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID,
};
use arora_behavior_tree::{
    load_behavior_tree_nodes_with, lower_behavior_tree, place_detached, PlacementScope,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

/// The clock's step and the damping time base, in seconds.
const DT: f64 = 1.0 / 30.0;

const MODULE: Uuid = Uuid::from_u128(0x5ca1_0000);
const X: Uuid = Uuid::from_u128(0x5ca1_0001);
const Y: Uuid = Uuid::from_u128(0x5ca1_0002);
const STATE: Uuid = Uuid::from_u128(0x5ca1_0003);
const OUT: Uuid = Uuid::from_u128(0x5ca1_0004);
const A: Uuid = Uuid::from_u128(0x5ca1_0005);
const B: Uuid = Uuid::from_u128(0x5ca1_0006);
const MIN: Uuid = Uuid::from_u128(0x5ca1_0007);
const MAX: Uuid = Uuid::from_u128(0x5ca1_0008);
const IN_MIN: Uuid = Uuid::from_u128(0x5ca1_0009);
const IN_MAX: Uuid = Uuid::from_u128(0x5ca1_000a);
const OUT_MIN: Uuid = Uuid::from_u128(0x5ca1_000b);
const OUT_MAX: Uuid = Uuid::from_u128(0x5ca1_000c);
const TARGET: Uuid = Uuid::from_u128(0x5ca1_000d);
const HALF_LIFE: Uuid = Uuid::from_u128(0x5ca1_000e);
const T: Uuid = Uuid::from_u128(0x5ca1_000f);
const FREQUENCY: Uuid = Uuid::from_u128(0x5ca1_0010);
const VALUE: Uuid = Uuid::from_u128(0x5ca1_0011);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    /// The `incremental` bench's functions, with its signature: `x` and `y`,
    /// and the mutable `state` and `out`, which the chains leave unbound.
    ChainAdd,
    ChainMul,
    ChainClamp,
    ChainRemap,
    Add,
    Mul,
    Clamp,
    Remap,
    Damp,
    Osc,
    /// Returns its argument: what a branch calls to write a value to a key.
    Publish,
}
impl Op {
    /// The parameters, in order, and whether each is mutable.
    fn parameters(self) -> &'static [(Uuid, &'static str, bool)] {
        match self {
            Op::ChainAdd | Op::ChainMul | Op::ChainClamp | Op::ChainRemap => &[
                (X, "x", false),
                (Y, "y", false),
                (STATE, "state", true),
                (OUT, "out", true),
            ],
            Op::Add | Op::Mul => &[(A, "a", false), (B, "b", false)],
            Op::Clamp => &[(X, "x", false), (MIN, "min", false), (MAX, "max", false)],
            Op::Remap => &[
                (X, "x", false),
                (IN_MIN, "in_min", false),
                (IN_MAX, "in_max", false),
                (OUT_MIN, "out_min", false),
                (OUT_MAX, "out_max", false),
            ],
            Op::Damp => &[
                (TARGET, "target", false),
                (HALF_LIFE, "half_life", false),
                (STATE, "state", true),
            ],
            Op::Osc => &[(T, "t", false), (FREQUENCY, "frequency", false)],
            Op::Publish => &[(VALUE, "value", false)],
        }
    }
}

/// A call's arguments as the host receives them: owned, through
/// [`CallBridge::arora_call`], or lent, through
/// [`CallBridge::arora_call_borrowed`]. The host reads either in place.
trait Arguments {
    fn get(&self, id: Uuid) -> Option<&Value>;
}
impl Arguments for Vec<StructureField> {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|field| field.id == id)
            .map(|field| &*field.value)
    }
}
impl Arguments for &[(Uuid, &Value)] {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|(parameter, _)| *parameter == id)
            .map(|(_, value)| *value)
    }
}

#[derive(Default)]
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    ops: HashMap<Uuid, Op>,
    module_calls: u64,
    dispatches: u64,
}
impl Bridge {
    fn index(&self) -> FunctionIndex {
        self.ops
            .iter()
            .map(|(id, op)| {
                let parameters = op.parameters();
                let function = ModuleFunction {
                    module_id: MODULE,
                    function_id: *id,
                    function_name: format!("{op:?}"),
                    function: Function {
                        parameter_ordering: parameters.iter().map(|(id, _, _)| *id).collect(),
                        parameters: parameters
                            .iter()
                            .map(|(id, name, mutable)| {
                                (
                                    *id,
                                    Parameter {
                                        name: (*name).into(),
                                        ty: FrozenTy::from(PrimitiveKind::F64),
                                        mutable: *mutable,
                                    },
                                )
                            })
                            .collect(),
                        return_ty: FrozenTy::from(PrimitiveKind::F64),
                    },
                };
                (*id, function)
            })
            .collect()
    }
    /// Answer a call of the function `id`: both entry points answer and count
    /// a call alike.
    fn answer(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: impl Arguments,
    ) -> Result<CallResult, CallError> {
        let op = self
            .ops
            .get(&id)
            .copied()
            .ok_or(CallError::FunctionNotFound { id })?;
        assert_eq!(module_id, Some(MODULE));
        let number = |id| match args.get(id) {
            Some(Value::F64(x)) => Some(*x),
            _ => None,
        };
        let v = |id| number(id).unwrap_or(0.0);
        let mut mutated = Vec::new();
        let ret = match op {
            Op::ChainAdd => v(X) + v(Y),
            Op::ChainMul => v(X) * v(Y),
            Op::ChainClamp => v(X).max(0.0).min(v(Y).abs() + 1.0),
            Op::ChainRemap => (v(X) - v(Y)) * 0.5,
            Op::Add => v(A) + v(B),
            Op::Mul => v(A) * v(B),
            Op::Clamp => v(MAX).min(v(MIN).max(v(X))),
            Op::Remap => {
                v(OUT_MIN)
                    + (v(X) - v(IN_MIN)) * (v(OUT_MAX) - v(OUT_MIN)) / (v(IN_MAX) - v(IN_MIN))
            }
            Op::Osc => (2.0 * PI * v(FREQUENCY) * v(T)).sin(),
            Op::Publish => v(VALUE),
            Op::Damp => {
                // The first call starts from its target; the state then eases
                // toward it and snaps once it is within 1e-4, so it settles.
                let target = v(TARGET);
                let state = number(STATE).filter(|x| !x.is_nan()).unwrap_or(target);
                let mut next = state + (target - state) * (1.0 - (-DT / v(HALF_LIFE)).exp2());
                if (target - next).abs() < 1e-4 {
                    next = target;
                }
                mutated.push(StructureField {
                    id: STATE,
                    value: Box::new(Value::F64(next)),
                });
                next
            }
        };
        self.module_calls += 1;
        Ok(CallResult {
            ret: Value::F64(ret),
            mutated,
        })
    }
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.answer(call.module_id, call.id, call.args)
    }
    fn arora_call_borrowed(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        self.answer(module_id, id, args)
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        self.next += 1;
        self.registered.insert(self.next, callable);
        CallableId { id: self.next }
    }
    fn arora_unregister_callable(&mut self, callable: &CallableId) {
        self.registered.remove(&callable.id);
    }
    fn arora_call_indirect(&mut self, callable: &CallableId) -> Result<Value, CallError> {
        self.dispatches += 1;
        let callable = self.registered[&callable.id].clone();
        callable.call(self)
    }
}

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
    arguments: Vec<(Uuid, Source)>,
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

/// The `incremental` bench's chains.
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
            op: [Op::ChainAdd, Op::ChainMul, Op::ChainClamp, Op::ChainRemap][i % 4],
            arguments: vec![(X, x), (Y, Source::Value(1.1))],
            publishes: (i + k >= n).then(|| outputs[i % k]),
        });
    }
    graph.bodies.push(Body {
        calls: (0..n).collect(),
        on: None,
    });
    graph
}

/// The JavaScript model's `seededRandom`: a 32-bit linear congruential
/// generator, scaled to [0, 1).
struct Random(u32);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0) / 4_294_967_296.0
    }
}

/// The JavaScript model's `generatedExample` for `clock` and `branches`,
/// drawing its random numbers in the same order.
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
                (Op::Add, vec![(A, x), (B, value(0.02))])
            } else if p < 0.60 {
                (Op::Mul, vec![(A, x), (B, value(0.5 + random.next() * 0.3))])
            } else if p < 0.75 {
                (
                    Op::Clamp,
                    vec![(X, x), (MIN, value(-1.0)), (MAX, value(1.0))],
                )
            } else if p < 0.95 {
                let remap = [(X, x), (IN_MIN, value(-2.0)), (IN_MAX, value(2.0))];
                let out = [(OUT_MIN, value(-0.5)), (OUT_MAX, value(0.5))];
                (Op::Remap, remap.into_iter().chain(out).collect())
            } else {
                (Op::Damp, vec![(TARGET, x), (HALF_LIFE, value(0.06))])
            };
            if clock_calls.contains(&i) {
                let frequency = 0.2 + clock_random.next() * 0.8;
                op = Op::Osc;
                arguments = vec![
                    (T, Source::Key(graph.time.unwrap())),
                    (FREQUENCY, value(frequency)),
                ];
            }
            for (_, source) in &arguments {
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
enum Scenario {
    Settled,
    OneInput,
    AllInputs,
    Clock,
    Branches,
    BranchesSwitching,
}
impl Scenario {
    const ALL: [Scenario; 6] = [
        Scenario::Settled,
        Scenario::OneInput,
        Scenario::AllInputs,
        Scenario::Clock,
        Scenario::Branches,
        Scenario::BranchesSwitching,
    ];
    fn name(self) -> &'static str {
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

#[derive(Clone, Copy, PartialEq)]
enum Form {
    AttachedNaive,
    Detached,
    AttachedIncremental,
    Placed,
}
impl Form {
    const ALL: [Form; 4] = [
        Form::AttachedNaive,
        Form::Detached,
        Form::AttachedIncremental,
        Form::Placed,
    ];
    fn name(self) -> &'static str {
        match self {
            Form::AttachedNaive => "attached-naive",
            Form::Detached => "detached",
            Form::AttachedIncremental => "attached-incremental",
            Form::Placed => "placed",
        }
    }
    fn detached(self) -> bool {
        matches!(self, Form::Detached | Form::Placed)
    }
}

fn call_id(c: usize) -> Uuid {
    Uuid::from_u128(0x4000_0000 + c as u128)
}
fn function_id(c: usize) -> Uuid {
    Uuid::from_u128(0x5000_0000 + c as u128)
}
/// The `publish` function a branch calls for call `c`.
fn publish_id(c: usize) -> Uuid {
    Uuid::from_u128(0x5100_0000 + c as u128)
}
fn key_variable(k: usize) -> Uuid {
    Uuid::from_u128(0x2000_0000 + k as u128)
}
fn local_variable(c: usize) -> Uuid {
    Uuid::from_u128(0x3000_0000 + c as u128)
}

/// The nodes of `graph` in `form`, root first, and the names of its store
/// variables.
fn build(graph: &Graph, form: Form) -> (Vec<Node>, HashMap<Uuid, String>) {
    let names: HashMap<_, _> = graph
        .keys
        .iter()
        .enumerate()
        .map(|(k, (name, _))| (key_variable(k), name.clone()))
        .collect();
    // A branch's graph is published by the branch, except by incremental
    // leaves, which the branch holds and which bind their keys themselves.
    let by_branch = graph.bodies[0].on.is_some() && !incremental_leaves(form);
    let bound = |c: usize| graph.calls[c].publishes.filter(|_| !by_branch);
    let source = |source: Source| match source {
        Source::Key(k) => Expression::VariableId(key_variable(k)),
        Source::Value(x) => Expression::Value(Value::F64(x)),
        Source::Call(c) if form.detached() => Expression::NodeArgument(NodeParameterId {
            node: call_id(c),
            parameter: _RET_PARAM_ID,
        }),
        Source::Call(c) => Expression::VariableId(match bound(c) {
            Some(k) => key_variable(k),
            None => local_variable(c),
        }),
    };
    let call = |c: usize| {
        let graph_call = &graph.calls[c];
        let mut arguments: HashMap<_, _> = graph_call
            .arguments
            .iter()
            .map(|(parameter, from)| (*parameter, source(*from)))
            .collect();
        if graph_call.op == Op::Damp {
            arguments.insert(STATE, Expression::Value(Value::F64(f64::NAN)));
        }
        match (bound(c), form.detached()) {
            (Some(k), _) => {
                arguments.insert(_RET_PARAM_ID, Expression::VariableId(key_variable(k)));
            }
            (None, false) => {
                arguments.insert(_RET_PARAM_ID, Expression::VariableId(local_variable(c)));
            }
            // An unpublished detached call runs because a sink reads it.
            (None, true) => {}
        }
        if form == Form::AttachedIncremental {
            arguments.insert(_ON_CHANGE_PARAM_ID, Expression::Value(Value::Boolean(true)));
        }
        Node {
            id: call_id(c),
            function: function_id(c),
            arguments,
            children: None,
        }
    };
    let mut tree = Builder::default();
    // What each branch ticks, or the lone graph: its own leaves on the
    // incremental executor, else only `RUN`.
    let bodies: Vec<_> = graph
        .bodies
        .iter()
        .map(|body| {
            let run = tree.push(RUN_FUNCTION_ID, HashMap::new(), None);
            if incremental_leaves(form) {
                tree.nodes.extend(body.calls.iter().map(|&c| call(c)));
                let leaves = body.calls.iter().map(|&c| call_id(c)).chain([run]);
                return tree.push(PARALLEL_FUNCTION_ID, HashMap::new(), Some(leaves.collect()));
            }
            if !by_branch {
                return run;
            }
            let published: Vec<_> = body
                .calls
                .iter()
                .filter_map(|&c| graph.calls[c].publishes.map(|k| (c, k)))
                .map(|(c, k)| {
                    let (value, key) = (
                        source(Source::Call(c)),
                        Expression::VariableId(key_variable(k)),
                    );
                    publish(&mut tree, form, c, value, key)
                })
                .chain([run])
                .collect();
            tree.push(PARALLEL_FUNCTION_ID, HashMap::new(), Some(published))
        })
        .collect();
    let mut root = if graph.bodies[0].on.is_none() {
        bodies[0]
    } else {
        let branches = graph
            .bodies
            .iter()
            .zip(bodies)
            .map(|(body, ticked)| {
                let on = Expression::VariableId(key_variable(body.on.unwrap()));
                let check = tree.push(
                    EQUAL_FUNCTION_ID,
                    HashMap::from([
                        (EQUAL_A_PARAM_ID, on),
                        (EQUAL_B_PARAM_ID, Expression::Value(Value::Boolean(true))),
                    ]),
                    None,
                );
                tree.push(SEQ_FUNCTION_ID, HashMap::new(), Some(vec![check, ticked]))
            })
            .collect();
        tree.push(FALLBACK_FUNCTION_ID, HashMap::new(), Some(branches))
    };
    let calls = 0..graph.calls.len();
    if form == Form::AttachedNaive {
        // A lone graph's body is its `RUN`: `PARALLEL[ leaf × N, RUN ]`.
        tree.nodes.extend(calls.clone().map(call));
        let children = calls.clone().map(call_id).chain([root]).collect();
        root = tree.push(PARALLEL_FUNCTION_ID, HashMap::new(), Some(children));
    }
    if form.detached() {
        tree.nodes.extend(calls.map(call));
    }
    let mut nodes = tree.nodes;
    let at = nodes.iter().position(|node| node.id == root).unwrap();
    let root = nodes.remove(at);
    nodes.insert(0, root);
    (nodes, names)
}

/// Whether the form holds each graph's leaves in its branch, as the
/// incremental executor's form does.
fn incremental_leaves(form: Form) -> bool {
    form == Form::AttachedIncremental
}

/// The leaf with which a branch publishes call `c`'s value to `key`:
/// placement's file uses the native `Copy`, the other forms call `publish`.
fn publish(tree: &mut Builder, form: Form, c: usize, value: Expression, key: Expression) -> Uuid {
    if form == Form::Placed {
        let arguments = [(COPY_VALUE_PARAM_ID, value), (COPY_OUT_PARAM_ID, key)];
        tree.push(COPY_FUNCTION_ID, HashMap::from(arguments), None)
    } else {
        let arguments = [(VALUE, value), (_RET_PARAM_ID, key)];
        tree.push(publish_id(c), HashMap::from(arguments), None)
    }
}

/// Control and check nodes, numbered from 1, beside the calls.
#[derive(Default)]
struct Builder {
    next: u128,
    nodes: Vec<Node>,
}
impl Builder {
    fn push(
        &mut self,
        function: Uuid,
        arguments: HashMap<Uuid, Expression>,
        children: Option<Vec<Uuid>>,
    ) -> Uuid {
        self.next += 1;
        let id = Uuid::from_u128(self.next);
        self.nodes.push(Node {
            id,
            function,
            arguments,
            children,
        });
        id
    }
}

struct Row {
    median: f64,
    quartiles: (f64, f64),
    calls: f64,
    dispatches: f64,
    published: Vec<Option<Value>>,
}

/// The page's quantile: linear between the two nearest order statistics.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let at = (sorted.len() - 1) as f64 * p;
    let (low, high) = (at.floor() as usize, at.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (at - low as f64)
}

fn measure(
    graph: &Graph,
    scenario: Scenario,
    form: Form,
    warm_up: usize,
    ticks: usize,
    samples: usize,
) -> Row {
    let store = SimpleDataStore::new();
    let slots: Vec<_> = graph
        .keys
        .iter()
        .map(|(name, initial)| {
            let slot = store.slot(&Key::from(name.as_str()));
            if let Some(value) = initial {
                slot.set(Some(value.clone())).unwrap();
            }
            slot
        })
        .collect();
    let mut written: Vec<_> = graph
        .keys
        .iter()
        .map(|(_, initial)| initial.clone())
        .collect();
    let mut bridge = Bridge {
        ops: graph
            .calls
            .iter()
            .enumerate()
            .map(|(c, call)| (function_id(c), call.op))
            .chain(
                (0..graph.calls.len())
                    .filter(|&c| graph.calls[c].publishes.is_some())
                    .map(|c| (publish_id(c), Op::Publish)),
            )
            .collect(),
        ..Bridge::default()
    };
    let (mut nodes, names) = build(graph, form);
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    if form == Form::Placed {
        nodes = place_detached(
            nodes,
            PlacementScope::Readers,
            &bridge.index(),
            &resolver,
            &names,
        )
        .unwrap();
    }
    let tree = load_behavior_tree_nodes_with(nodes, &resolver, &names).unwrap();
    let tree = lower_behavior_tree(tree, &bridge.index(), &mut bridge, false).unwrap();
    tree.set_incremental(matches!(form, Form::AttachedIncremental | Form::Placed));
    // Write a key only when its value changes, as a host would.
    let mut step = 0;
    let mut tick = |bridge: &mut Bridge, step: &mut usize| {
        scenario.changes(graph, *step, |k, value| {
            if written[k].as_ref() != Some(&value) {
                slots[k].set(Some(value.clone())).unwrap();
                written[k] = Some(value);
            }
        });
        *step += 1;
        black_box(tree.tick(bridge).unwrap());
    };
    for _ in 0..warm_up {
        tick(&mut bridge, &mut step);
    }
    bridge.module_calls = 0;
    bridge.dispatches = 0;
    let mut times = Vec::new();
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..ticks {
            tick(&mut bridge, &mut step);
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / ticks as f64);
    }
    times.sort_by(f64::total_cmp);
    let divisor = (ticks * samples) as f64;
    Row {
        median: quantile(&times, 0.5),
        quartiles: (quantile(&times, 0.25), quantile(&times, 0.75)),
        calls: bridge.module_calls as f64 / divisor,
        dispatches: bridge.dispatches as f64 / divisor,
        published: graph
            .calls
            .iter()
            .filter_map(|call| call.publishes)
            .map(|k| slots[k].get())
            .collect(),
    }
}

fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let ticks: usize = args.first().map_or(500, |arg| arg.parse().expect("ticks"));
    let samples: usize = args.get(1).map_or(7, |arg| arg.parse().expect("samples"));
    let only = args.get(2).map(|arg| arg.as_str());
    assert!(
        ticks > 0 && samples > 0 && samples % 2 == 1,
        "positive ticks and odd samples"
    );
    let warm_up = 200;
    println!("ticks={ticks}, samples={samples}, warm-up={warm_up}; median and quartile times, mean counts per tick");
    println!("| N | scenario | form | us/tick | q1 | q3 | module calls/tick | dispatches/tick |");
    println!("|---:|---|---|---:|---:|---:|---:|---:|");
    for n in [10, 30, 100, 300, 1000] {
        for scenario in Scenario::ALL {
            let graph = scenario.graph(n);
            let mut expected = None;
            for form in Form::ALL {
                let cell = format!("{n}/{}/{}", scenario.name(), form.name());
                if only.is_some_and(|only| only != cell) {
                    continue;
                }
                let row = measure(&graph, scenario, form, warm_up, ticks, samples);
                let compared =
                    !(scenario == Scenario::BranchesSwitching && form == Form::AttachedNaive);
                if compared {
                    match &expected {
                        Some(expected) => assert_eq!(
                            &row.published,
                            expected,
                            "{n} {} {}: published values differ",
                            scenario.name(),
                            form.name()
                        ),
                        None => expected = Some(row.published.clone()),
                    }
                }
                println!(
                    "| {n} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |",
                    scenario.name(),
                    form.name(),
                    row.median,
                    row.quartiles.0,
                    row.quartiles.1,
                    row.calls,
                    row.dispatches
                );
            }
        }
    }
}
