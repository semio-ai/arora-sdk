//! A public, deterministic workload with graph sinks and event-driven branches.
use crate::model::Workload;
use serde_json::{json, Value};

pub const TICKS: usize = 160;
pub const WARM_UP: usize = 16;

pub fn json() -> Value {
    let key = |k| json!({"key": k});
    let node = |n| json!({"node": n});
    let value = |v: f64| json!({"value": v});
    let args = |a: Vec<Value>| {
        a.into_iter()
            .enumerate()
            .map(|(i, s)| json!([format!("a{i}"), s]))
            .collect::<Vec<_>>()
    };
    let call = |op, a: Vec<Value>, publishes: Option<usize>| json!({"op": op, "args": args(a), "publishes": publishes});
    let mut calls = vec![
        call("sum", vec![key(1), value(0.3)], None),
        call("product", vec![node(0), value(0.8)], None),
        call("osc", vec![value(2.0), value(0.1), key(0)], None),
        call("mix", vec![node(1), node(2)], None),
        call("damp", vec![key(3), value(0.12)], Some(5)),
        call("sum", vec![node(4), node(1)], Some(6)),
    ];
    for i in 6..18 {
        calls.push(call(
            "sum",
            vec![node(if i == 6 { 3 } else { i - 1 }), value(0.02)],
            None,
        ));
    }
    let action = |op, a: Vec<Value>, outputs: Vec<usize>, state| json!({"k": "call", "op": op, "args": args(a), "outputs": outputs.into_iter().enumerate().map(|(i, k)| json!([format!("o{i}"), k])).collect::<Vec<_>>(), "state": state});
    let branch = |mode, children: Vec<Value>| json!({"k": "seq", "children": [{"k": "equal", "key": 2, "value": mode}, {"k": "parallel", "children": children}]});
    let tree = json!({"k": "parallel", "children": [
        {"k": "fallback", "children": [
            branch(0, vec![action("follow", vec![node(3)], vec![3], 0), json!({"k": "publish", "node": 17, "key": 4}), json!({"k": "calls", "calls": (6..18).collect::<Vec<_>>()})]),
            branch(1, vec![action("pulse", vec![key(0), value(0.04), value(0.10), value(0.2), value(0.8), value(0.025), value(7.0)], vec![3], 3), action("wander", vec![key(0), value(0.05), value(0.08), value(9.0), value(-0.4), value(0.4)], vec![4], 2)]),
            branch(2, vec![action("hold", vec![key(0), value(0.0), value(0.6)], vec![3], 2), action("keyframes", vec![key(0), value(1.0), value(0.0), value(0.2), value(0.2), value(0.9)], vec![4], 3)]),
            branch(3, vec![action("set", vec![value(-0.25)], vec![3], 0), action("above", vec![key(1), value(-1.0)], vec![], 0), json!({"k": "publish", "node": 17, "key": 4})])
        ]}, {"k": "run"}
    ]});
    let events: Vec<_> = (0..TICKS)
        .flat_map(|tick| {
            [
                json!([tick, 1, ((tick / 7) % 11) as f64 / 10.0]),
                json!([tick, 2, tick / 40]),
            ]
        })
        .collect();
    json!({"name": "synthetic", "dt": 1.0 / 60.0, "ticks": TICKS, "warmUp": WARM_UP, "clock": 0,
        "keys": [
            {"name": "arora/time", "initial": 0.0}, {"name": "input", "initial": 0.0}, {"name": "mode", "initial": 0.0},
            {"name": "target", "initial": 0.0}, {"name": "branch/output", "initial": 0.0},
            {"name": "graph/damped", "initial": null}, {"name": "graph/mixed", "initial": null}
        ], "calls": calls, "tree": tree, "events": events,
        "phases": {"names": ["follow", "pulse", "hold", "set"], "runs": [[0, 0], [40, 1], [80, 2], [120, 3]]}})
}

pub fn workload() -> Workload {
    Workload::parse(&json().to_string()).expect("valid synthetic workload")
}
