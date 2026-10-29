//! A graph and a diff read from each of their forms, written back, and the
//! references that do not resolve.

use std::collections::HashMap;

use arora_behavior::FunctionIndex;
use uuid::Uuid;

use super::*;
use crate::call_expression_tests::{f64_ty, function, index, parameter, MATH};
use crate::text::{parse, resolve, Loaded};

/// The text, its graph and a diff to it, as `docs/text.md` shows them.
fn documented() -> (String, String, String) {
    let doc = include_str!("../../docs/text.md");
    let (_, section) = doc
        .split_once("\n## Readable graphs and diffs\n")
        .expect("the section on readable graphs");
    let section = section.split("\n## ").next().unwrap_or(section);
    let mut blocks = section.split("\n```").skip(1).step_by(2).map(|block| {
        let (_, content) = block.split_once('\n').expect("a fenced block");
        format!("{content}\n")
    });
    let mut block = || blocks.next().expect("a text, a graph and a diff");
    (block(), block(), block())
}

/// The documented graph, as readable JSON.
const READABLE_JSON: &str = r#"{
  "root": "watch",
  "nodes": [
    {"id": "watch", "function": "fallback", "children": ["calm", "done"]},
    {"id": "calm", "function": "sequence", "children": ["near", "settle"]},
    {"id": "near", "function": "below", "bind": {
      "x": {"port": {"node": "cm", "port": "_ret"}},
      "limit": {"value": {"f64": 30.0}}}},
    {"id": "settle", "function": "smooth", "bind": {
      "x": {"port": {"node": "cm", "port": "_ret"}},
      "alpha": {"value": {"f64": 0.5}},
      "state": {"key": ".s"}}},
    {"id": "done", "function": "succeed"},
    {"id": "cm", "function": "clamp", "bind": {
      "x": {"key": "sensor/distance"},
      "lo": {"value": {"f64": 0.0}}}},
    {"id": "level", "function": "add", "bind": {
      "_ret": {"key": "face/level"},
      "a": {"port": {"node": "cm", "port": "_ret"}},
      "b": {"value": {"f64": 1.0}}}}
  ]
}"#;

/// The documented text edited: `near`'s limit changed, `settle`'s state
/// unbound, `cm` given a ceiling, `level` replaced by `pulse`.
const EDITED: &str = "\
watch: fallback
  calm: sequence
    near: below(x: @cm._ret, limit: 20)
    settle: smooth(x: @cm._ret, alpha: 0.5)
  done: succeed

cm: clamp(x: {sensor/distance}, lo: 0, hi: 50)
pulse: {face/pulse} = mul(a: @cm._ret, b: 2)
";

/// The documented diff, as readable JSON.
const DIFF_JSON: &str = r#"{
  "remove": ["level"],
  "set": [{"id": "pulse", "function": "mul", "bind": {
    "_ret": {"key": "face/pulse"},
    "a": {"port": {"node": "cm", "port": "_ret"}},
    "b": {"value": {"f64": 2.0}}}}],
  "unbind": [{"node": "settle", "port": "state"}],
  "bind": [
    {"node": "near", "port": "limit", "source": {"value": {"f64": 20.0}}},
    {"node": "cm", "port": "hi", "source": {"value": {"f64": 50.0}}}
  ]
}"#;

fn loaded(text: &str) -> Loaded {
    let parsed = parse(text);
    assert_eq!(parsed.diagnostics, [], "{text}");
    let loaded = resolve(&parsed.document, &index());
    assert_eq!(loaded.diagnostics, [], "{text}");
    loaded
}

fn node_names(loaded: &Loaded) -> HashMap<Uuid, String> {
    loaded
        .nodes
        .iter()
        .filter_map(|node| Some((node.id, node.name.clone()?)))
        .collect()
}

fn yaml<T: serde::de::DeserializeOwned>(text: &str) -> T {
    serde_yaml::from_str(text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> T {
    serde_json::from_str(text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// A graph read from its readable YAML, its readable JSON, its JSON by ids
/// (with no function index) and its text is one graph.
#[test]
fn a_graph_is_one_in_every_form() {
    let functions = index();
    let (behavior, readable_yaml, _) = documented();
    let text = loaded(&behavior).graph;
    let by_ids = serde_json::to_string(&text).expect("the graph serializes");
    let forms = [
        (
            "readable YAML",
            yaml::<Graph>(&readable_yaml).resolve(&functions),
        ),
        (
            "readable JSON",
            json::<Graph>(READABLE_JSON).resolve(&functions),
        ),
        (
            "JSON by ids",
            json::<Graph>(&by_ids).resolve(&FunctionIndex::new()),
        ),
    ];
    for (form, graph) in forms {
        assert_eq!(graph, Ok(text.clone()), "{form}");
    }
}

/// A diff read from its readable YAML, its readable JSON and its JSON by ids
/// is the diff between the texts before and after the edit.
#[test]
fn a_diff_is_one_in_every_form() {
    let functions = index();
    let (behavior, _, diff_yaml) = documented();
    let base = loaded(&behavior).graph;
    let text = base.diff(&loaded(EDITED).graph);
    let by_ids = serde_json::to_string(&text).expect("the diff serializes");
    let forms = [
        (
            "readable YAML",
            yaml::<GraphDiff>(&diff_yaml).resolve(&base, &functions),
        ),
        (
            "readable JSON",
            json::<GraphDiff>(DIFF_JSON).resolve(&base, &functions),
        ),
        (
            "JSON by ids",
            json::<GraphDiff>(&by_ids).resolve(&base, &FunctionIndex::new()),
        ),
    ];
    for (form, diff) in forms {
        assert_eq!(diff, Ok(text.clone()), "{form}");
    }
}

/// A graph and a diff written readable name every reference the text names,
/// and read back to themselves.
#[test]
fn a_graph_and_a_diff_written_readable_read_back() {
    let functions = index();
    let (behavior, readable_yaml, diff_yaml) = documented();
    let (before, after) = (loaded(&behavior), loaded(EDITED));
    let mut nodes = node_names(&before);
    nodes.extend(node_names(&after));
    let names = Names {
        functions: Some(&functions),
        variables: None,
        nodes: Some(&nodes),
    };

    let written = Graph::from_graph(&before.graph, &names);
    assert_eq!(written, yaml::<Graph>(&readable_yaml));
    let read: Graph = yaml(&serde_yaml::to_string(&written).expect("it serializes"));
    assert_eq!(read.resolve(&functions), Ok(before.graph.clone()));

    let diff = before.graph.diff(&after.graph);
    let written = GraphDiff::from_diff(&diff, &before.graph, &names);
    assert_eq!(written, yaml::<GraphDiff>(&diff_yaml));
    let read: GraphDiff = yaml(&serde_yaml::to_string(&written).expect("it serializes"));
    assert_eq!(read.resolve(&before.graph, &functions), Ok(diff));
}

fn errors<T: std::fmt::Debug>(resolved: Result<T, Vec<ReferenceError>>) -> Vec<String> {
    resolved
        .expect_err("some references do not resolve")
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// A name that names nothing is reported where the file writes it.
#[test]
fn an_unresolved_name_is_reported_where_it_is_written() {
    let graph: Graph = yaml(
        "
root: nowhere
nodes:
- id: watch
  function: sequence
  children: [cm, ghost]
- id: cm
  function: clamp
  bind:
    x: {select: {source: {port: {node: gone, port: _ret}}, path: '.0'}}
    width: {value: {f64: 1.0}}
- id: odd
  function: nosuch
  bind:
    x: {key: a}
- id: anon
  function: 00000000-0000-4000-8000-00000000abcd
  bind:
    x: {key: a}
",
    );
    assert_eq!(
        errors(graph.resolve(&index())),
        [
            "nodes[2].function: no function is named `nosuch`",
            "nodes[0].children[1]: no node is named `ghost`",
            "nodes[1].bind.width: `clamp` has no parameter `width`",
            "nodes[1].bind.x.select.source.port.node: no node is named `gone`",
            "nodes[3].bind.x: `x` names no port: the node's function is not known, \
             its ports are named by their ids",
            "root: no node is named `nowhere`",
        ]
    );

    let base = loaded(&documented().0).graph;
    let diff: GraphDiff = yaml(
        "
remove: [ghost]
unbind:
- {node: near, port: width}
bind:
- {node: level, port: hi, source: {value: {f64: 1.0}}}
",
    );
    assert_eq!(
        errors(diff.resolve(&base, &index())),
        [
            "remove[0]: no node is named `ghost`",
            "unbind[0].port: `below` has no parameter `width`",
            "bind[0].port: `add` has no parameter `hi`",
        ]
    );
}

/// A name naming several functions, or several ports, is reported where
/// the file writes it, and so is a port bound twice.
#[test]
fn an_ambiguous_name_is_reported_where_it_is_written() {
    let mut functions = index();
    let other = Uuid::from_u128(0x7700);
    let twins = Uuid::from_u128(0x7701);
    for (id, function) in [
        function(other, Uuid::from_u128(0x7702), "add", vec![]),
        function(
            other,
            twins,
            "twins",
            vec![
                (Uuid::from_u128(0x7711), parameter("x", f64_ty(), false)),
                (Uuid::from_u128(0x7712), parameter("x", f64_ty(), false)),
                (Uuid::from_u128(0x7713), parameter("_ret", f64_ty(), false)),
            ],
        ),
    ] {
        functions.insert(id, function);
    }
    let graph: Graph = yaml(&format!(
        "
nodes:
- id: sum
  function: add
- id: pair
  function: twins
  bind:
    x: {{key: a}}
    _ret: {{key: b}}
- id: cm
  function: clamp
  bind:
    lo: {{key: a}}
    {}: {{key: b}}
",
        crate::call_expression_tests::CLAMP_LO
    ));
    let several = format!("{MATH}, {other}");
    assert_eq!(
        errors(graph.resolve(&functions)),
        [
            format!(
                "nodes[0].function: `add` is a function of several modules: \
                 write `<module>::add`, the module one of {several}"
            ),
            "nodes[1].bind._ret: `_ret` names several ports of `twins`: \
             the port is named by its id"
                .to_string(),
            "nodes[1].bind.x: `x` names several ports of `twins`: the port is named by its id"
                .to_string(),
            format!(
                "nodes[2].bind.lo: port {} is bound twice",
                crate::call_expression_tests::CLAMP_LO
            ),
        ]
    );
}
