//! `cargo bench -p arora-behavior-tree --offline --bench incremental -- [ticks] [samples]`
//! prints median microseconds per tick after 50 warm-up ticks, plus module calls
//! and engine indirect dispatches per measured tick. Defaults: 500 ticks, 7 samples.
//! Sizes are 10, 100, 1000; inputs feed independent chains of add/mul/clamp/remap
//! calls. `one-input` changes one chain, `all-inputs` changes all chains.
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

use arora_behavior_tree::nodes::{PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID};
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
    let mut nodes = vec![Node {
        id: id(1),
        function: if detached {
            RUN_FUNCTION_ID
        } else {
            PARALLEL_FUNCTION_ID
        },
        children: (!detached).then(|| (0..=n).map(|i| id(2 + i)).collect()),
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
    tree.set_incremental(mode == "attached-incremental");
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
            for mode in ["attached-naive", "attached-incremental", "detached"] {
                let (time, calls, dispatches, outputs) = measure(n, mode, scenario, ticks, samples);
                if let Some(expected) = &expected {
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
