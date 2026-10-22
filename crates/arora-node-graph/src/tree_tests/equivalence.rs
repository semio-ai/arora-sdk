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

/// Whether `spec` has a node evaluating a URDF, which fails on both engines
/// without the `urdf_ik` feature.
fn needs_urdf(spec: &GraphSpec) -> bool {
    use crate::types::NodeType;
    spec.nodes.iter().any(|node| {
        matches!(
            node.kind,
            NodeType::UrdfFk | NodeType::UrdfIkPosition | NodeType::UrdfIkPose
        )
    })
}

/// One tick of `reference` evaluating `reference_spec` and `tree` its
/// counterpart `tree_spec`, over the same inputs and clock, and the two
/// compared port by port and write by write. Returns whether the tick
/// evaluates: an IK target out of a chain's reach fails it on both.
fn tick_both(
    name: &str,
    tick: usize,
    inputs: &[(String, Option<Value>)],
    (reference, reference_spec): (&mut GraphRuntime, &GraphSpec),
    (tree, tree_spec): (&mut TreeRuntime, &GraphSpec),
) -> bool {
    reference.dt = 1.0 / 60.0;
    tree.dt = reference.dt;
    reference.t += reference.dt;
    tree.t += tree.dt;
    for (i, (input, default)) in inputs.iter().enumerate() {
        let path = TypedPath::parse(input).expect("a path");
        reference.set_input(path.clone(), staged(default, i, tick), None);
        tree.set_input(path, staged(default, i, tick), None);
    }
    let expected = evaluate_all(reference, reference_spec);
    let actual = tree.evaluate_spec(tree_spec);
    match (&expected, &actual) {
        (Ok(()), Ok(())) => {}
        (Err(_), Err(_)) => return false,
        _ => panic!("{name}, tick {tick}: the reference gives {expected:?}, the tree {actual:?}"),
    }
    for (node, ports) in &reference.outputs {
        for (port, value) in ports {
            let got = tree.outputs.get(node).and_then(|ports| ports.get(port));
            let got =
                got.unwrap_or_else(|| panic!("{name}, tick {tick}: the tree has no {node}.{port}"));
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
    true
}

#[test]
fn every_fixture_computes_on_the_tree_what_it_computes_on_the_reference() {
    for (name, _) in crate::fixtures::ALL {
        let (spec, inputs) = fixture(name);
        let spec = spec.with_cache();
        let mut reference = GraphRuntime::default();
        let mut tree = TreeRuntime::default();
        let compared = (0..40)
            .filter(|tick| {
                tick_both(
                    name,
                    *tick,
                    &inputs,
                    (&mut reference, &spec),
                    (&mut tree, &spec),
                )
            })
            .count();
        assert!(
            compared > 0 || (needs_urdf(&spec) && !cfg!(feature = "urdf_ik")),
            "{name}: no tick evaluates"
        );
    }
}

/// The parameters of a node that take any number, which an editor's slider
/// moves.
const CONTINUOUS: &[&str] = &[
    "frequency",
    "phase",
    "min",
    "max",
    "x",
    "y",
    "z",
    "in_min",
    "in_max",
    "out_min",
    "out_max",
    "in_low",
    "in_anchor",
    "in_high",
    "out_low",
    "out_anchor",
    "out_high",
    "bone1",
    "bone2",
    "bone3",
    "lacunarity",
    "persistence",
    "stiffness",
    "damping",
    "mass",
    "half_life",
    "max_rate",
];

/// `node`'s parameters with every continuous one, and a constant's value,
/// scaled by `factor`.
fn edited(node: &crate::types::NodeSpec, factor: f64) -> crate::types::NodeParams {
    let mut json = serde_json::to_value(&node.params).expect("params serialize");
    if let serde_json::Value::Object(fields) = &mut json {
        for (field, value) in fields.iter_mut() {
            match value {
                serde_json::Value::Number(number) if CONTINUOUS.contains(&field.as_str()) => {
                    *value = serde_json::json!(number.as_f64().unwrap_or(0.0) * factor);
                }
                _ if field == "value" && node.kind == crate::types::NodeType::Constant => {
                    scale(value, factor)
                }
                _ => {}
            }
        }
    }
    serde_json::from_value(json).expect("params deserialize")
}

/// Every fixture with its parameters edited halfway through, in place on
/// the tree and in the spec the reference evaluates: the two compute the
/// same, each node keeping its state through the edit, and the tree is not
/// built again.
#[test]
fn a_parameter_edited_in_place_computes_what_the_reference_computes_from_the_edited_spec() {
    let mut edits = 0;
    for (name, _) in crate::fixtures::ALL {
        let (spec, inputs) = fixture(name);
        let spec = spec.with_cache();
        let mut reference = GraphRuntime::default();
        let mut tree = TreeRuntime::default();
        let (mut reference_spec, mut tree_spec) = (spec.clone(), spec.clone());
        for tick in 0..40 {
            if tick == 20 {
                for (at, node) in spec.nodes.iter().enumerate() {
                    let params = edited(node, 1.1);
                    let in_place = tree.set_params(&node.id, &params).expect("an edit");
                    assert!(
                        in_place,
                        "{name}: the edit of {}'s parameters is made in place",
                        node.id
                    );
                    edits += 1;
                    tree_spec.nodes[at].params = params.clone();
                    reference_spec.nodes[at].params = params;
                }
                // The reference plans the edited spec again, as its host
                // does for an edit it cannot tell is a value's; its nodes
                // keep their state.
                reference_spec = reference_spec.with_cache();
            }
            tick_both(
                name,
                tick,
                &inputs,
                (&mut reference, &reference_spec),
                (&mut tree, &tree_spec),
            );
        }
        assert_eq!(tree.loads(), 1, "{name}: the tree is built once");
    }
    assert!(edits > 0);
}
