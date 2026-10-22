//! `cargo bench -p arora-behavior-tree --offline --bench incremental -- [ticks] [samples]`
//! prints median microseconds per tick after 50 warm-up ticks, plus module calls
//! and engine indirect dispatches per measured tick. Defaults: 500 ticks, 7 samples.
//! Sizes are 10, 100, 1000; inputs feed independent chains of add/mul/clamp/remap
//! calls. `one-input` changes one chain, `all-inputs` changes all chains.
//! `attached-plain-incremental` enables the flag on the attached tree without
//! `_on_change`, measuring the incremental cost for existing effectful trees.
//!
//! `attached-guarded` is the attached tree on today's executor (flag off) with
//! each leaf wrapped as `FALLBACK[SEQ[Equal(a: 0, b: 1), leaf], SUCCEED]`, the
//! pattern a tree uses to skip an unchanged node without the flag. `Equal` on
//! two different literals always fails, so no leaf is called: the row is the
//! cost of the guard alone on a settled graph, four native nodes ticked per
//! graph node and no module call. A literal guard cannot follow an input
//! change, so this form runs in `settled` only, and it is left out of the
//! output comparison because its leaves never publish.
//!
//! Attached leaves bind `_ret` to a tree-local port and mirror the computed
//! value through a mutable `out` parameter to a store key. Detached nodes bind
//! `_ret` directly to those keys (sink publication). The host ignores `out`'s
//! incoming value. Store publication is included in all timings. Counters are
//! collected without recording call arguments; this measures a native test
//! bridge, not wasm execution. Compare counts as well as times.
mod support {
    pub mod incremental;
}

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

use arora_behavior_tree::nodes::{
    EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, FALLBACK_FUNCTION_ID,
    PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID,
};
use arora_behavior_tree::schema::{
    Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID,
};
use arora_behavior_tree::{load_behavior_tree_nodes_with, lower_behavior_tree};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key};
use arora_types::value::Value;
use support::incremental::{Bridge, Op, OUT, X, Y};
use uuid::Uuid;

fn id(n: usize) -> Uuid {
    Uuid::from_u128(n as u128)
}
fn variable(n: usize) -> Expression {
    Expression::VariableId(id(n))
}
/// First id of the `attached-guarded` wrapper nodes: graph node `i` takes
/// `GUARD + 4 * i` to `GUARD + 4 * i + 3`, above every other id in the bench.
const GUARD: usize = 1_000_000;
fn measure(
    n: usize,
    mode: &str,
    scenario: &str,
    ticks: usize,
    samples: usize,
) -> (f64, f64, f64, Vec<Option<Value>>) {
    let k = 2.max(n / 10);
    let store = SimpleDataStore::new();
    let mut names = HashMap::new();
    let inputs: Vec<_> = (0..k)
        .map(|i| {
            let key = format!("input/{i}");
            names.insert(id(60000 + i), key.clone());
            let slot = store.slot(&Key::from(key));
            slot.set(Some(Value::F64(0.0))).unwrap();
            slot
        })
        .collect();
    let outputs: Vec<_> = (0..n)
        .map(|i| {
            let key = format!("output/{i}");
            names.insert(id(90000 + i), key.clone());
            store.slot(&Key::from(key))
        })
        .collect();
    let detached = mode == "detached";
    let guarded = mode == "attached-guarded";
    let top = |i: usize| id(if guarded { GUARD + 4 * i } else { 2 + i });
    let mut nodes = vec![Node {
        id: id(1),
        function: if detached {
            RUN_FUNCTION_ID
        } else {
            PARALLEL_FUNCTION_ID
        },
        children: (!detached).then(|| (0..n).map(top).chain([id(2 + n)]).collect()),
        ..Node::default()
    }];
    let mut bridge = Bridge::default();
    bridge.count_only = true;
    for i in 0..n {
        let function = id(100000 + i);
        bridge
            .ops
            .insert(function, [Op::Add, Op::Mul, Op::Clamp, Op::Remap][i % 4]);
        let input = if i < k {
            variable(60000 + i)
        } else {
            Expression::NodeArgument(NodeParameterId {
                node: id(2 + i - k),
                parameter: _RET_PARAM_ID,
            })
        };
        let mut arguments = HashMap::from([
            (X, input),
            (Y, Expression::Value(Value::F64(1.1))),
            (
                _RET_PARAM_ID,
                variable(if detached { 90000 + i } else { 30000 + i }),
            ),
        ]);
        if !detached {
            arguments.insert(OUT, variable(90000 + i));
        }
        if mode == "attached-incremental" {
            arguments.insert(_ON_CHANGE_PARAM_ID, Expression::Value(Value::Boolean(true)));
        }
        nodes.push(Node {
            id: id(2 + i),
            function,
            arguments,
            children: None,
        });
        if guarded {
            let fallback = GUARD + 4 * i;
            let (seq, equal, succeed) = (fallback + 1, fallback + 2, fallback + 3);
            nodes.extend([
                Node {
                    id: id(fallback),
                    function: FALLBACK_FUNCTION_ID,
                    children: Some(vec![id(seq), id(succeed)]),
                    ..Node::default()
                },
                Node {
                    id: id(seq),
                    function: SEQ_FUNCTION_ID,
                    children: Some(vec![id(equal), id(2 + i)]),
                    ..Node::default()
                },
                Node {
                    id: id(equal),
                    function: EQUAL_FUNCTION_ID,
                    arguments: HashMap::from([
                        (EQUAL_A_PARAM_ID, Expression::Value(Value::F64(0.0))),
                        (EQUAL_B_PARAM_ID, Expression::Value(Value::F64(1.0))),
                    ]),
                    ..Node::default()
                },
                Node {
                    id: id(succeed),
                    function: SUCCEED_FUNCTION_ID,
                    ..Node::default()
                },
            ]);
        }
    }
    if !detached {
        nodes.push(Node {
            id: id(2 + n),
            function: RUN_FUNCTION_ID,
            ..Node::default()
        });
    }
    let tree =
        load_behavior_tree_nodes_with(nodes, &|name| Some(store.slot(&Key::from(name))), &names)
            .unwrap();
    let tree = lower_behavior_tree(tree, &bridge.index(), &mut bridge, false).unwrap();
    tree.set_incremental(matches!(
        mode,
        "attached-incremental" | "attached-plain-incremental"
    ));
    let update = |step: usize| {
        let changed = match scenario {
            "settled" => 0,
            "one-input" => 1,
            "all-inputs" => k,
            _ => unreachable!(),
        };
        for input in inputs.iter().take(changed) {
            input.set(Some(Value::F64((step % 17) as f64))).unwrap();
        }
    };
    for step in 0..50 {
        update(step);
        black_box(tree.tick(&mut bridge).unwrap());
    }
    let mut times = Vec::new();
    let mut calls = 0;
    let mut dispatches = 0;
    for sample in 0..samples {
        bridge.reset_counts();
        let start = Instant::now();
        for step in 0..ticks {
            update(50 + sample * ticks + step);
            black_box(tree.tick(&mut bridge).unwrap());
        }
        times.push(start.elapsed().as_secs_f64() * 1e6 / ticks as f64);
        calls += bridge.module_calls;
        dispatches += bridge.dispatches;
    }
    times.sort_by(f64::total_cmp);
    let divisor = (ticks * samples) as f64;
    (
        times[samples / 2],
        calls as f64 / divisor,
        dispatches as f64 / divisor,
        outputs.iter().map(|slot| slot.get()).collect(),
    )
}
fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let ticks: usize = args.first().map_or(500, |arg| arg.parse().expect("ticks"));
    let samples: usize = args.get(1).map_or(7, |arg| arg.parse().expect("samples"));
    assert!(
        ticks > 0 && samples > 0 && samples % 2 == 1,
        "positive ticks and odd samples"
    );
    println!("ticks={ticks}, samples={samples}, warm-up=50; median time, mean counts per tick");
    println!("| N | scenario | form | us/tick | module calls/tick | dispatches/tick |");
    println!("|---:|---|---|---:|---:|---:|");
    for n in [10, 100, 1000] {
        for scenario in ["settled", "one-input", "all-inputs"] {
            let mut expected = None;
            for mode in [
                "attached-naive",
                "attached-plain-incremental",
                "attached-incremental",
                "detached",
                "attached-guarded",
            ] {
                let guarded = mode == "attached-guarded";
                if guarded && scenario != "settled" {
                    continue;
                }
                let (time, calls, dispatches, outputs) = measure(n, mode, scenario, ticks, samples);
                if guarded {
                    assert_eq!(calls, 0.0, "{n} {scenario} {mode}: a guarded leaf ran");
                } else if let Some(expected) = &expected {
                    assert_eq!(&outputs, expected, "{n} {scenario} {mode}");
                } else {
                    expected = Some(outputs);
                }
                println!(
                    "| {n} | {scenario} | {mode} | {time:.3} | {calls:.3} | {dispatches:.3} |"
                );
            }
        }
    }
}
