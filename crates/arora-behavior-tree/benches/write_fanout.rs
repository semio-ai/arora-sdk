//! What a tick costs when a module function's array output is written to N
//! store keys, for each way a tree can say which value goes to which key.
//!
//! Every case ticks `Sequence[produce, …]`: `produce(values: &mut [Value])` is
//! a module leaf, served natively by the bench's bridge, that writes N values
//! which all change every tick, as a `Value::ArrayValue` of `F64`s — the form
//! of the animation module's `step_values`; what follows it writes them to the keys
//! `face/k0 … face/k{N-1}` of a `SimpleDataStore` with one subscriber, drained
//! every tick. The cases:
//!
//! - `baseline`: `produce` alone, its output in a tree-local cell, nothing
//!   written to the store — the cost every case shares;
//! - `write_keys`: `WriteKeys(keys, values ← produce.values)`, the key table a
//!   literal bound at lowering;
//! - `unpack`: `Unpack(values ← produce.values, out_i → {face/ki})`, a node
//!   with N variadic output ports, each linked to a variable, bound at
//!   lowering;
//! - `unpack_lookup`: the same node, resolving each port's cell by id on every
//!   tick;
//! - `set_select`: N `Set(variable → {face/ki}, value ← produce.values.i)`
//!   nodes, each reading its element through a selection on the link;
//! - `spread`: `produce.values` bound to the tuple of variables `({face/k0},
//!   …)`, no node — the module's write of its mutable parameter is the fan-out;
//! - `set_spread`: one `Set(variable → ({face/k0}, …), value ←
//!   produce.values)`, the same tuple binding on a node's output.
//!
//! Four pipelines smooth the values (`Smooth`, exponential, alpha 0.5, state
//! in the node) before they reach `face/k*`:
//!
//! - `pipe_store`: `Unpack` to the store keys `raw/k*`, then one `Smooth` per
//!   key from `{raw/ki}` to `{face/ki}` — every stage through the store;
//! - `pipe_local`: one `Smooth` over the array into a tree-local variable,
//!   then `Unpack` to `{face/k*}`;
//! - `pipe_packed`: one `Smooth` over the array into the one store key
//!   `face/packed`;
//! - `pipe_perkey`: `Unpack` to N tree-local variables, then one `Smooth` per
//!   key to `{face/ki}` — local, but per key.
//!
//! Each tree case runs with the keys in the store (`store`) and in tree-local
//! cells (`local`), the tree's own share of the cost. Two cases without a
//! tree give the store's share alone: `store_slots` writes the N values
//! through N slots, as every tree case does, and `store_batch` as one change.
//!
//! Without arguments it runs every case for N in 1, 10, 30 and 100 and prints,
//! per tick, the instructions the process retired (macOS `proc_pid_rusage`),
//! the heap allocations and the wall time. With `<case> <n> <ticks> [store |
//! local]` it runs that one case `ticks` times after its setup, for an
//! instruction counter: under callgrind, a run of T ticks minus a run of 0 is
//! T ticks, setup excluded.
//!
//! `cargo bench -p arora-behavior-tree --bench write_fanout [-- <case> <n> <ticks> [<sink>]]`

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use arora_behavior_tree::arora_generated::behavior_tree::status::Status;
use arora_behavior_tree::nodes::{
    unpack_out_param_id, SEQ_FUNCTION_ID, SET_VALUE_FUNCTION_ID, SET_VALUE_VALUE_PARAM_ID,
    SET_VALUE_VARIABLE_PARAM_ID, UNPACK_FUNCTION_ID, UNPACK_LOOKUP_FUNCTION_ID,
    UNPACK_VALUES_PARAM_ID, WRITE_KEYS_FUNCTION_ID, WRITE_KEYS_KEYS_PARAM_ID,
    WRITE_KEYS_VALUES_PARAM_ID,
};
use arora_behavior_tree::nodes::{
    SMOOTH_ALPHA_PARAM_ID, SMOOTH_FUNCTION_ID, SMOOTH_OUT_PARAM_ID, SMOOTH_VALUE_PARAM_ID,
};
use arora_behavior_tree::schema::{Expression, Node, NodeParameterId};
use arora_behavior_tree::{
    load_behavior_tree_nodes_with, lower_behavior_tree, LoweredTree, ModuleFunction,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, Slot, StateChange, Subscription};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

/// Counts the heap allocations of the process.
struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const CASES: [&str; 11] = [
    "baseline",
    "write_keys",
    "unpack",
    "unpack_lookup",
    "set_select",
    "spread",
    "set_spread",
    "pipe_store",
    "pipe_local",
    "pipe_packed",
    "pipe_perkey",
];
const SIZES: [usize; 4] = [1, 10, 30, 100];

const PRODUCE: Uuid = Uuid::from_u128(0xF00D);
const PRODUCE_VALUES: Uuid = Uuid::from_u128(0xF00E);
const ROOT: Uuid = Uuid::from_u128(0x1);
const PRODUCE_NODE: Uuid = Uuid::from_u128(0x2);
const WRITER_NODE: Uuid = Uuid::from_u128(0x3);

fn key(i: usize) -> String {
    format!("face/k{i}")
}

fn variable(i: usize) -> Uuid {
    Uuid::from_u128(0xA000 + i as u128)
}

fn set_node(i: usize) -> Uuid {
    Uuid::from_u128(0xB000 + i as u128)
}

/// The intermediate store key of a value, before smoothing.
fn raw_key(i: usize) -> String {
    format!("raw/k{i}")
}

/// The variable of [`raw_key`]`(i)`.
fn raw_variable(i: usize) -> Uuid {
    Uuid::from_u128(0xC000 + i as u128)
}

/// A tree-local variable, by index: it has no name, so no store key.
fn local_variable(i: usize) -> Uuid {
    Uuid::from_u128(0xD000 + i as u128)
}

/// The tree-local variable holding the smoothed array.
const LOCAL_ARRAY: Uuid = Uuid::from_u128(0xE000);
/// The store key holding the whole smoothed array.
const PACKED_KEY: &str = "face/packed";
const PACKED_VARIABLE: Uuid = Uuid::from_u128(0xE001);
const SMOOTH_NODE: Uuid = Uuid::from_u128(0x4);

fn smooth_node(i: usize) -> Uuid {
    Uuid::from_u128(0xB100 + i as u128)
}

fn smooth(id: Uuid, value: Expression, out: Uuid) -> Node {
    node(
        id,
        SMOOTH_FUNCTION_ID,
        vec![
            (SMOOTH_VALUE_PARAM_ID, value),
            (SMOOTH_ALPHA_PARAM_ID, Expression::Value(Value::F64(0.5))),
            (SMOOTH_OUT_PARAM_ID, Expression::VariableId(out)),
        ],
    )
}

fn unpack(values: Expression, out: impl Fn(usize) -> Uuid, n: usize) -> Node {
    node(
        WRITER_NODE,
        UNPACK_FUNCTION_ID,
        std::iter::once((UNPACK_VALUES_PARAM_ID, values))
            .chain((0..n).map(|i| (unpack_out_param_id(i), Expression::VariableId(out(i)))))
            .collect(),
    )
}

/// `produce(values: &mut [Value]) -> Status`.
fn function_index() -> Rc<HashMap<Uuid, ModuleFunction>> {
    Rc::new(HashMap::from([(
        PRODUCE,
        ModuleFunction {
            module_id: Uuid::from_u128(0xF000),
            function_id: PRODUCE,
            function_name: "produce".to_string(),
            function: Function {
                // The bridge does not decode its arguments: the tree reads a
                // parameter's type only for whether it is optional.
                parameters: HashMap::from([(
                    PRODUCE_VALUES,
                    Parameter {
                        name: "values".to_string(),
                        ty: FrozenTy::from(PrimitiveKind::F64),
                        mutable: true,
                    },
                )]),
                parameter_ordering: vec![PRODUCE_VALUES],
                return_ty: FrozenTy::from(PrimitiveKind::U8),
            },
        },
    )]))
}

/// Serves `produce` natively: every call writes `n` values that all differ
/// from the previous call's.
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next_id: u64,
    n: usize,
    calls: u64,
}

impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        if call.id != PRODUCE {
            return Err(CallError::FunctionNotFound { id: call.id });
        }
        self.calls += 1;
        let base = self.calls as f64;
        Ok(CallResult {
            ret: Status::Success.into(),
            mutated: vec![StructureField {
                id: PRODUCE_VALUES,
                value: Box::new(Value::ArrayValue(
                    (0..self.n).map(|i| Value::F64(base + i as f64)).collect(),
                )),
            }],
        })
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        let id = self.next_id;
        self.next_id += 1;
        self.registered.insert(id, callable);
        CallableId { id }
    }
    fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
        self.registered.remove(&callable_id.id);
    }
    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        let callable = self.registered[&callable_id.id].clone();
        callable.call(self)
    }
}

fn node(id: Uuid, function: Uuid, arguments: Vec<(Uuid, Expression)>) -> Node {
    Node {
        id,
        function,
        arguments: arguments.into_iter().collect(),
        children: None,
    }
}

/// The tree of `case` over `n` keys: root first.
fn tree_nodes(case: &str, n: usize) -> Vec<Node> {
    let produced = || {
        Expression::NodeArgument(NodeParameterId {
            node: PRODUCE_NODE,
            parameter: PRODUCE_VALUES,
        })
    };
    let local_output = Expression::Value(Value::ArrayValue(vec![Value::F64(0.0); n]));
    let (produce_values, writers): (Expression, Vec<Node>) = match case {
        "baseline" => (local_output, vec![]),
        "write_keys" => (
            local_output,
            vec![node(
                WRITER_NODE,
                WRITE_KEYS_FUNCTION_ID,
                vec![
                    (
                        WRITE_KEYS_KEYS_PARAM_ID,
                        Expression::Value(Value::ArrayString((0..n).map(key).collect())),
                    ),
                    (WRITE_KEYS_VALUES_PARAM_ID, produced()),
                ],
            )],
        ),
        "unpack" | "unpack_lookup" => (
            local_output,
            vec![node(
                WRITER_NODE,
                if case == "unpack" {
                    UNPACK_FUNCTION_ID
                } else {
                    UNPACK_LOOKUP_FUNCTION_ID
                },
                std::iter::once((UNPACK_VALUES_PARAM_ID, produced()))
                    .chain(
                        (0..n)
                            .map(|i| (unpack_out_param_id(i), Expression::VariableId(variable(i)))),
                    )
                    .collect(),
            )],
        ),
        "set_select" => (
            local_output,
            (0..n)
                .map(|i| {
                    node(
                        set_node(i),
                        SET_VALUE_FUNCTION_ID,
                        vec![
                            (
                                SET_VALUE_VARIABLE_PARAM_ID,
                                Expression::VariableId(variable(i)),
                            ),
                            (
                                SET_VALUE_VALUE_PARAM_ID,
                                Expression::Select {
                                    source: Box::new(produced()),
                                    path: Key::new(format!(".{i}")),
                                },
                            ),
                        ],
                    )
                })
                .collect(),
        ),
        "spread" => (
            Expression::Spread((0..n).map(|i| Some(variable(i))).collect()),
            vec![],
        ),
        // A: every stage through the store.
        "pipe_store" => (
            local_output,
            std::iter::once(unpack(produced(), raw_variable, n))
                .chain((0..n).map(|i| {
                    smooth(
                        smooth_node(i),
                        Expression::VariableId(raw_variable(i)),
                        variable(i),
                    )
                }))
                .collect(),
        ),
        // B: smoothed as one array in a tree-local variable, then unpacked.
        "pipe_local" => (
            local_output,
            vec![
                smooth(SMOOTH_NODE, produced(), LOCAL_ARRAY),
                unpack(Expression::VariableId(LOCAL_ARRAY), variable, n),
            ],
        ),
        // C: smoothed as one array, written to one store key.
        "pipe_packed" => (
            local_output,
            vec![smooth(SMOOTH_NODE, produced(), PACKED_VARIABLE)],
        ),
        // D: unpacked to tree-local variables, smoothed per key.
        "pipe_perkey" => (
            local_output,
            std::iter::once(unpack(produced(), local_variable, n))
                .chain((0..n).map(|i| {
                    smooth(
                        smooth_node(i),
                        Expression::VariableId(local_variable(i)),
                        variable(i),
                    )
                }))
                .collect(),
        ),
        "set_spread" => (
            local_output,
            vec![node(
                WRITER_NODE,
                SET_VALUE_FUNCTION_ID,
                vec![
                    (
                        SET_VALUE_VARIABLE_PARAM_ID,
                        Expression::Spread((0..n).map(|i| Some(variable(i))).collect()),
                    ),
                    (SET_VALUE_VALUE_PARAM_ID, produced()),
                ],
            )],
        ),
        other => panic!("unknown case {other}; the cases are {CASES:?}"),
    };
    let mut root = node(ROOT, SEQ_FUNCTION_ID, vec![]);
    root.children = Some(
        std::iter::once(PRODUCE_NODE)
            .chain(writers.iter().map(|writer| writer.id))
            .collect(),
    );
    let mut nodes = vec![
        root,
        node(
            PRODUCE_NODE,
            PRODUCE,
            vec![(PRODUCE_VALUES, produce_values)],
        ),
    ];
    nodes.extend(writers);
    nodes
}

/// Where the written keys live.
#[derive(Clone, Copy, PartialEq)]
enum Sink {
    /// In a `SimpleDataStore` with one subscriber, drained every tick.
    Store,
    /// In tree-local cells: the tree's own share of the cost.
    Local,
}

/// A case set up and ticked once: the tree lowered onto its bridge, the store
/// and its subscriber.
struct Setup {
    tree: LoweredTree,
    bridge: Bridge,
    store: SimpleDataStore,
    feed: Subscription,
    n: usize,
    sink: Sink,
}

impl Setup {
    fn new(case: &str, n: usize, sink: Sink) -> Self {
        let store = SimpleDataStore::new();
        let feed = store.subscribe();
        let resolver_store = store.clone();
        let resolver =
            move |name: &str| (sink == Sink::Store).then(|| resolver_store.slot(&Key::from(name)));
        let names: HashMap<Uuid, String> = (0..n)
            .flat_map(|i| [(variable(i), key(i)), (raw_variable(i), raw_key(i))])
            .chain([(PACKED_VARIABLE, PACKED_KEY.to_string())])
            .collect();
        let tree = load_behavior_tree_nodes_with(tree_nodes(case, n), &resolver, &names)
            .expect("the tree loads");
        let mut bridge = Bridge {
            registered: HashMap::new(),
            next_id: 0,
            n,
            calls: 0,
        };
        let tree = lower_behavior_tree(tree, function_index(), &mut bridge, false)
            .expect("the tree lowers");
        let mut setup = Setup {
            tree,
            bridge,
            store,
            feed,
            n,
            sink,
        };
        setup.tick();
        setup.check(case);
        setup
    }

    fn tick(&mut self) {
        let status = self.tree.tick(&mut self.bridge).expect("the tick runs");
        assert_eq!(status, Status::Success);
        for change in self.feed.try_iter() {
            std::hint::black_box(change);
        }
    }

    /// Every key holds the last tick's value, or none in the baseline and with
    /// tree-local cells.
    fn check(&self, case: &str) {
        if case.starts_with("pipe_") {
            return self.check_pipeline(case);
        }
        let keys: Vec<Key> = (0..self.n).map(|i| Key::from(key(i))).collect();
        let base = self.bridge.calls as f64;
        let written = case != "baseline" && self.sink == Sink::Store;
        let expected: Vec<Option<Value>> = (0..self.n)
            .map(|i| written.then_some(Value::F64(base + i as f64)))
            .collect();
        assert_eq!(self.store.read(&keys), expected, "{case} writes every key");
    }
}

impl Setup {
    /// The smoothed values reach their keys: one per key, or the whole array
    /// under one key (`pipe_packed`); none with tree-local cells.
    fn check_pipeline(&self, case: &str) {
        let store = self.sink == Sink::Store;
        if case == "pipe_packed" {
            let packed = self.store.read(&[Key::from(PACKED_KEY)]).remove(0);
            match packed {
                Some(Value::ArrayValue(values)) if store => assert_eq!(values.len(), self.n),
                None if !store => {}
                other => panic!("{case}: the packed key holds {other:?}"),
            }
            return;
        }
        let keys: Vec<Key> = (0..self.n).map(|i| Key::from(key(i))).collect();
        for (i, value) in self.store.read(&keys).into_iter().enumerate() {
            assert!(
                matches!((&value, store), (Some(Value::F64(_)), true) | (None, false)),
                "{case}: key {i} holds {value:?}"
            );
        }
    }
}

/// The store's share alone, without a tree: N values written each tick
/// through N slots (`store_slots`) or as one change (`store_batch`).
struct Raw {
    batch: bool,
    store: SimpleDataStore,
    feed: Subscription,
    slots: Vec<Box<dyn Slot>>,
    keys: Vec<Key>,
    calls: u64,
}

impl Raw {
    fn new(case: &str, n: usize) -> Self {
        let store = SimpleDataStore::new();
        let feed = store.subscribe();
        let keys: Vec<Key> = (0..n).map(|i| Key::from(key(i))).collect();
        Raw {
            batch: case == "store_batch",
            slots: keys.iter().map(|key| store.slot(key)).collect(),
            store,
            feed,
            keys,
            calls: 0,
        }
    }

    fn tick(&mut self) {
        self.calls += 1;
        let base = self.calls as f64;
        if self.batch {
            let change = StateChange {
                set: self
                    .keys
                    .iter()
                    .enumerate()
                    .map(|(i, key)| (key.clone(), Some(Value::F64(base + i as f64))))
                    .collect(),
                unset: Default::default(),
            };
            self.store.write(change).expect("the store writes");
        } else {
            for (i, slot) in self.slots.iter().enumerate() {
                slot.set(Some(Value::F64(base + i as f64)))
                    .expect("the slot writes");
            }
        }
        for change in self.feed.try_iter() {
            std::hint::black_box(change);
        }
    }
}

/// A tree case, or a store case without a tree.
enum Runner {
    Tree(Setup),
    Raw(Raw),
}

impl Runner {
    fn new(case: &str, n: usize, sink: Sink) -> Self {
        match case {
            "store_slots" | "store_batch" => Runner::Raw(Raw::new(case, n)),
            _ => Runner::Tree(Setup::new(case, n, sink)),
        }
    }

    fn tick(&mut self) {
        match self {
            Runner::Tree(setup) => setup.tick(),
            Runner::Raw(raw) => raw.tick(),
        }
    }

    fn check(&self, case: &str) {
        if let Runner::Tree(setup) = self {
            setup.check(case);
        }
    }
}

/// The instructions this process retired so far, where the system counts them.
#[cfg(target_os = "macos")]
fn instructions() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: the buffer is a `rusage_info_v4`, the flavor asked for.
    let read = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as libc::c_int,
            libc::RUSAGE_INFO_V4,
            usage.as_mut_ptr().cast(),
        )
    };
    // SAFETY: a zero return filled the buffer.
    (read == 0).then(|| unsafe { usage.assume_init() }.ri_instructions)
}

#[cfg(not(target_os = "macos"))]
fn instructions() -> Option<u64> {
    None
}

/// `ticks` ticks of a set-up case: a function of its own, so a profiler can
/// attribute them.
#[inline(never)]
fn run_ticks(setup: &mut Runner, ticks: usize) {
    for _ in 0..ticks {
        setup.tick();
    }
}

fn measure(case: &str, n: usize, sink: Sink) -> (Option<f64>, f64, f64) {
    const WARMUP: usize = 100;
    const TICKS: usize = 2_000;
    let mut setup = Runner::new(case, n, sink);
    run_ticks(&mut setup, WARMUP);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let retired = instructions();
    let start = Instant::now();
    run_ticks(&mut setup, TICKS);
    let elapsed = start.elapsed();
    let retired = retired
        .zip(instructions())
        .map(|(before, after)| (after - before) as f64 / TICKS as f64);
    let allocations = (ALLOCATIONS.load(Ordering::Relaxed) - allocations) as f64 / TICKS as f64;
    setup.check(case);
    (
        retired,
        allocations,
        elapsed.as_nanos() as f64 / TICKS as f64,
    )
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let sink = |arg: Option<&String>| match arg.map(String::as_str) {
        None | Some("store") => Sink::Store,
        Some("local") => Sink::Local,
        Some(other) => panic!("the sinks are store and local, not {other}"),
    };
    if let [case, n, ticks, rest @ ..] = &args[..] {
        let mut setup = Runner::new(case, n.parse().expect("n is a count"), sink(rest.first()));
        run_ticks(&mut setup, ticks.parse().expect("ticks is a count"));
        setup.check(case);
        return;
    }
    println!(
        "{:<14} {:<6} {:>4} {:>12} {:>12} {:>10}",
        "case", "sink", "n", "instr/tick", "allocs/tick", "ns/tick"
    );
    let grid = CASES
        .iter()
        .flat_map(|case| [(*case, Sink::Store), (*case, Sink::Local)])
        .chain([("store_slots", Sink::Store), ("store_batch", Sink::Store)]);
    for (case, sink) in grid {
        for n in SIZES {
            let (retired, allocations, nanos) = measure(case, n, sink);
            let retired = retired.map_or("-".to_string(), |r| format!("{r:.0}"));
            let sink = if sink == Sink::Store {
                "store"
            } else {
                "local"
            };
            println!("{case:<14} {sink:<6} {n:>4} {retired:>12} {allocations:>12.1} {nanos:>10.0}");
        }
    }
}
