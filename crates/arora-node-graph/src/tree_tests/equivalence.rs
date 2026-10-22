//! Every fixture evaluated on both engines, over the same inputs and clock:
//! the reference evaluator and the tree compute the same value on every port
//! of every node, and make the same writes, at every tick.
use arora_shape::{TypedPath, Value};

use crate::tree_runtime::TreeRuntime;
use crate::types::GraphSpec;
use crate::{evaluate_all, GraphRuntime};

/// The fixture `name`'s graph, and its input nodes' paths with the value
/// each reads when nothing is staged.
fn fixture(name: &str) -> (GraphSpec, Vec<(String, Option<Value>)>) {
    let document: serde_json::Value = crate::fixtures::spec(name).expect("the fixture parses");
    let mut spec = match document.get("spec") {
        Some(spec) => spec.clone(),
        None => document.clone(),
    };
    arora_shape::json::normalize_graph_spec_value(&mut spec).expect("the fixture normalizes");
    let spec: GraphSpec = serde_json::from_value(spec).expect("a graph");
    let mut inputs: Vec<(String, Option<Value>)> = spec
        .nodes
        .iter()
        .filter(|node| node.kind == crate::types::NodeType::Input)
        .filter_map(|node| {
            Some((
                node.params.path.as_ref()?.to_string(),
                node.params.value.clone(),
            ))
        })
        .collect();
    inputs.sort_by(|a, b| a.0.cmp(&b.0));
    inputs.dedup_by(|a, b| a.0 == b.0);
    (spec, inputs)
}

/// Every `f32` of `json`, the serde form of a value, scaled by `factor`.
fn scale(json: &mut serde_json::Value, factor: f64) {
    match json {
        serde_json::Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                match (key.as_str(), field) {
                    ("f32", serde_json::Value::Number(number)) => {
                        *number =
                            serde_json::Number::from_f64(number.as_f64().unwrap_or(0.0) * factor)
                                .unwrap_or_else(|| serde_json::Number::from(0));
                    }
                    ("f32s", serde_json::Value::Array(numbers)) => {
                        for number in numbers {
                            *number = serde_json::json!(number.as_f64().unwrap_or(0.0) * factor);
                        }
                    }
                    (_, field) => scale(field, factor),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|item| scale(item, factor)),
        _ => {}
    }
}

/// The value staged on the `i`-th input at tick `tick`: its default, every
/// number of it moving; a moving number for an input with no default.
fn staged(default: &Option<Value>, i: usize, tick: usize) -> Value {
    let factor = 1.0 + 0.05 * (tick as f64 * 0.37 + i as f64).sin();
    match default {
        Some(default) => {
            let mut json = serde_json::to_value(default).expect("a value serializes");
            scale(&mut json, factor);
            serde_json::from_value(json).expect("a value deserializes")
        }
        None => Value::F32(factor as f32 * 2.0 - 2.0),
    }
}

fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::F32(a), Value::F32(b)) => (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-6,
        (a, b) => a == b,
    }
}

#[test]
fn every_fixture_computes_on_the_tree_what_it_computes_on_the_reference() {
    for (name, _) in crate::fixtures::ALL {
        let (spec, inputs) = fixture(name);
        let spec = spec.with_cache();
        let mut reference = GraphRuntime::default();
        let mut tree = TreeRuntime::default();
        let mut compared = 0;
        for tick in 0..40 {
            reference.dt = 1.0 / 60.0;
            tree.dt = reference.dt;
            reference.t += reference.dt;
            tree.t += tree.dt;
            for (i, (input, default)) in inputs.iter().enumerate() {
                let path = TypedPath::parse(input).expect("a path");
                reference.set_input(path.clone(), staged(default, i, tick), None);
                tree.set_input(path, staged(default, i, tick), None);
            }
            let expected = evaluate_all(&mut reference, &spec);
            let actual = tree.evaluate_spec(&spec);
            match (&expected, &actual) {
                (Ok(()), Ok(())) => {}
                // Both fail alike: an IK target out of the chain's reach.
                (Err(_), Err(_)) => continue,
                _ => panic!(
                    "{name}, tick {tick}: the reference gives {expected:?}, the tree {actual:?}"
                ),
            }
            compared += 1;
            for (node, ports) in &reference.outputs {
                for (port, value) in ports {
                    let got = tree.outputs.get(node).and_then(|ports| ports.get(port));
                    let got = got.unwrap_or_else(|| {
                        panic!("{name}, tick {tick}: the tree has no {node}.{port}")
                    });
                    assert!(
                        close(&value.value, &got.value),
                        "{name}, tick {tick}: {node}.{port} is {:?} on the reference, {:?} on the tree",
                        value.value,
                        got.value
                    );
                    assert_eq!(
                        value.shape, got.shape,
                        "{name}, tick {tick}: {node}.{port}'s shape"
                    );
                }
            }
            let writes = |batch: &arora_shape::WriteBatch| {
                batch
                    .iter()
                    .map(|op| (op.path.to_string(), op.value.clone()))
                    .collect::<Vec<_>>()
            };
            let (expected, actual) = (writes(&reference.writes), writes(&tree.writes));
            assert_eq!(
                expected.len(),
                actual.len(),
                "{name}, tick {tick}: the writes"
            );
            for ((path, value), (got_path, got)) in expected.iter().zip(&actual) {
                assert_eq!(path, got_path, "{name}, tick {tick}");
                assert!(
                    close(value, got),
                    "{name}, tick {tick}: {path} is {value:?}, then {got:?}"
                );
            }
        }
        assert!(compared > 0, "{name}: no tick evaluates");
    }
}
