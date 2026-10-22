//! What a tick costs that writes a module's array output to N store keys,
//! through `Unpack` (one output port per key, bound at lowering) or `Scatter`
//! (the keys as data, resolved while the tree ticks).
//!
//! Every case ticks `Sequence(produce, …)`: `produce(values: &mut [Value])` is
//! a module leaf, served by the bench's bridge, that writes N values which all
//! change every tick, as a `Value::ArrayValue` of `F64`s — the form of the
//! animation module's `step_values` — into a tree-local variable. What
//! follows it writes them to the keys `face/k0 … face/k{N-1}` of a
//! `SimpleDataStore`. The cases:
//!
//! - `baseline`: `produce` alone, the cost every case shares;
//! - `unpack`: `Unpack(values, out_i → {face/ki})`, N at most 64;
//! - `scatter`: `Scatter(values, keys, within = "face")`, the keys a literal
//!   array, equal from one tick to the next;
//! - `scatter_rekeyed`: the same, `produce` also writing the keys, which
//!   alternate between two tables every tick, so every tick resolves them.
//!
//! `cargo bench -p arora-behavior-tree --bench scatter -- <case> <n> <ticks>`
//! runs `<ticks>` ticks after the setup and a first tick.
//!
//! Count instructions, not time: run it under callgrind, or difference the
//! process's instructions retired (`/usr/bin/time -l` on macOS) between
//! `<ticks>` and 0 ticks. The time it prints is indicative only.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use arora_behavior::{FunctionIndex, ModuleFunction, Status};
use arora_behavior_tree::nodes::{
    SCATTER_FUNCTION_ID, SCATTER_KEYS_PARAM_ID, SCATTER_VALUES_PARAM_ID, SCATTER_WITHIN_PARAM_ID,
    SEQ_FUNCTION_ID, UNPACK_FUNCTION_ID, UNPACK_OUT_PARAM_IDS, UNPACK_VALUES_PARAM_ID,
};
use arora_behavior_tree::schema::{Expression, Node, NodeParameterId};
use arora_behavior_tree::variable::VariableResolver;
use arora_behavior_tree::{load_behavior_tree_nodes_with, lower_behavior_tree};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, Slot};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

const PRODUCE: Uuid = Uuid::from_u128(0xF00D);
const PRODUCE_VALUES: Uuid = Uuid::from_u128(0xF00E);
const PRODUCE_KEYS: Uuid = Uuid::from_u128(0xF00F);
const ROOT: Uuid = Uuid::from_u128(0x1);
const PRODUCE_NODE: Uuid = Uuid::from_u128(0x2);
const WRITER_NODE: Uuid = Uuid::from_u128(0x3);

fn key(table: u64, i: usize) -> String {
    format!("face/t{table}/k{i}")
}

fn variable(i: usize) -> Uuid {
    Uuid::from_u128(0xA000 + i as u128)
}

/// `produce(values: &mut [Value], keys: &mut [String]) -> Status`.
fn function_index() -> FunctionIndex {
    let mutable = |name: &str| Parameter {
        name: name.to_string(),
        ty: FrozenTy::from(PrimitiveKind::F64),
        mutable: true,
    };
    HashMap::from([(
        PRODUCE,
        ModuleFunction {
            module_id: Uuid::from_u128(0xF000),
            function_id: PRODUCE,
            function_name: "produce".to_string(),
            function: Function {
                // The bridge does not decode its arguments: the tree reads a
                // parameter's type only for whether it is optional.
                parameters: HashMap::from([
                    (PRODUCE_VALUES, mutable("values")),
                    (PRODUCE_KEYS, mutable("keys")),
                ]),
                parameter_ordering: vec![PRODUCE_VALUES, PRODUCE_KEYS],
                return_ty: FrozenTy::from(PrimitiveKind::U8),
            },
        },
    )])
}

/// Serves `produce`: every call writes `n` values that all differ from the
/// previous call's and, when `rekey`, the keys of the other table.
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next_id: u64,
    n: usize,
    rekey: bool,
    calls: u64,
}

impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        if call.id != PRODUCE {
            return Err(CallError::FunctionNotFound { id: call.id });
        }
        self.calls += 1;
        let base = self.calls as f64;
        let mut mutated = vec![StructureField {
            id: PRODUCE_VALUES,
            value: Box::new(Value::ArrayValue(
                (0..self.n).map(|i| Value::F64(base + i as f64)).collect(),
            )),
        }];
        if self.rekey {
            mutated.push(StructureField {
                id: PRODUCE_KEYS,
                value: Box::new(Value::ArrayString(
                    (0..self.n).map(|i| key(self.calls % 2, i)).collect(),
                )),
            });
        }
        Ok(CallResult {
            ret: Status::Success.into(),
            mutated,
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

/// The tree of `case` over `n` keys, root first.
fn tree_nodes(case: &str, n: usize) -> Vec<Node> {
    let produced = |parameter| {
        Expression::NodeArgument(NodeParameterId {
            node: PRODUCE_NODE,
            parameter,
        })
    };
    let keys = Value::ArrayString((0..n).map(|i| key(0, i)).collect());
    let writers =
        match case {
            "baseline" => vec![],
            "unpack" => {
                assert!(n <= UNPACK_OUT_PARAM_IDS.len(), "Unpack has 64 ports");
                vec![node(
                    WRITER_NODE,
                    UNPACK_FUNCTION_ID,
                    std::iter::once((UNPACK_VALUES_PARAM_ID, produced(PRODUCE_VALUES)))
                        .chain((0..n).map(|i| {
                            (UNPACK_OUT_PARAM_IDS[i], Expression::VariableId(variable(i)))
                        }))
                        .collect(),
                )]
            }
            "scatter" | "scatter_rekeyed" => vec![node(
                WRITER_NODE,
                SCATTER_FUNCTION_ID,
                vec![
                    (SCATTER_VALUES_PARAM_ID, produced(PRODUCE_VALUES)),
                    (
                        SCATTER_KEYS_PARAM_ID,
                        if case == "scatter" {
                            Expression::Value(keys.clone())
                        } else {
                            produced(PRODUCE_KEYS)
                        },
                    ),
                    (
                        SCATTER_WITHIN_PARAM_ID,
                        Expression::Value(Value::String("face".to_string())),
                    ),
                ],
            )],
            other => panic!("unknown case {other}"),
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
            vec![
                (
                    PRODUCE_VALUES,
                    Expression::Value(Value::ArrayValue(vec![Value::F64(0.0); n])),
                ),
                (PRODUCE_KEYS, Expression::Value(keys)),
            ],
        ),
    ];
    nodes.extend(writers);
    nodes
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let case = args.first().map_or("scatter", String::as_str);
    let n: usize = args.get(1).map_or(64, |n| n.parse().expect("<n>"));
    let ticks: u32 = args.get(2).map_or(10_000, |t| t.parse().expect("<ticks>"));

    let store = SimpleDataStore::new();
    let handle = store.clone();
    let resolver: Rc<VariableResolver<'static>> =
        Rc::new(move |name: &str| -> Option<Box<dyn Slot>> { Some(handle.slot(&Key::from(name))) });
    let names: HashMap<Uuid, String> = (0..n).map(|i| (variable(i), key(0, i))).collect();
    let tree = load_behavior_tree_nodes_with(tree_nodes(case, n), &*resolver, &names)
        .expect("the tree loads")
        .with_key_resolver(resolver);
    let mut bridge = Bridge {
        registered: HashMap::new(),
        next_id: 0,
        n,
        rekey: case == "scatter_rekeyed",
        calls: 0,
    };
    let functions = function_index();
    let tree = lower_behavior_tree(tree, &functions, &mut bridge, false).expect("the tree lowers");
    let mut tick = || {
        let status = tree.tick(&mut bridge).expect("the tick runs");
        assert_eq!(status, Status::Success);
    };
    tick();
    let start = Instant::now();
    for _ in 0..ticks {
        tick();
    }
    let elapsed = start.elapsed();

    let table = if case == "scatter_rekeyed" {
        bridge.calls % 2
    } else {
        0
    };
    let written = store.read(&[Key::from(key(table, n - 1))]);
    if case != "baseline" {
        assert_eq!(
            written,
            vec![Some(Value::F64((bridge.calls + n as u64 - 1) as f64))],
            "{case} writes every key"
        );
    }
    println!(
        "{case} n={n}: {ticks} ticks, {:.0} ns/tick (indicative)",
        elapsed.as_nanos() as f64 / f64::from(ticks.max(1))
    );
}
