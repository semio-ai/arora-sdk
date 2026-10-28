//! The text language: parsing and its recovery, formatting, resolution, the
//! display read back, and texts run end to end on the fake engine of
//! [`call_expression_tests`](crate::call_expression_tests).

use std::collections::HashMap;

use arora_behavior::graph::{Graph, LinkSource, Port};
use arora_behavior::{FunctionIndex, Status};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::gen_uuid_from_str;
use arora_types::value::Value;
use uuid::Uuid;

use super::*;
use crate::call_expression_tests::{
    centimetres, f, f64_ty, function, index, node, parameter, Calls, Engine, ADD_A, BELOW,
    BELOW_LIMIT, BELOW_X, CLAMP_HI, CLAMP_LO, CLAMP_X, MATH, MUL, MUL_A, MUL_B, SMOOTH_STATE,
};
use crate::display::Names;
use crate::graph::build_behavior_tree;
use crate::nodes::{
    EQUAL_FUNCTION_ID, FALLBACK_FUNCTION_ID, SEQ_FUNCTION_ID, SEQ_STAR_CURRENT_INDEX_PARAM_ID,
    SET_STR_VALUE_PARAM_ID, SET_STR_VARIABLE_PARAM_ID,
};
use crate::schema::_RET_PARAM_ID;
use crate::{load_behavior_tree_nodes, load_behavior_tree_text, BehaviorTree, BehaviorTreeRuntime};

/// The contract's example, over the fake engine's functions.
const EXAMPLE: &str = "\
# comment to end of line
fallback                                   # the first top-level node is the root
  sequence
    equal(a: {person/visible}, b: true)
    set_string(variable: {.mood}, value: \"happy\")
    below(x: @cm, limit: 30)
    smooth(x: @raw, alpha: 0.2, state: @s.state)
  succeed

raw: mul(a: {sensor/distance}, b: 100)
cm: clamp(x: @raw, lo: 0)
level: {face/level} = add(a: @cm, b: 1)
s: smooth(x: @cm, alpha: 0.5, state: 0)
sway: {rig/arm_wave} = mul(a: add(a: {arora/time}, b: 1.6), b: 0.6)
";

fn parsed(text: &str) -> Document {
    let parsed = parse(text);
    assert_eq!(parsed.diagnostics, [], "{text}");
    parsed.document
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect()
}

fn loaded(text: &str) -> Loaded {
    loaded_with(text, &index())
}

fn loaded_with(text: &str, functions: &FunctionIndex) -> Loaded {
    let loaded = resolve(&parsed(text), functions);
    assert_eq!(
        errors(&loaded.diagnostics),
        Vec::<&Diagnostic>::new(),
        "{text}"
    );
    loaded
}

/// The text a span covers.
fn at(text: &str, span: Span) -> &str {
    &text[span.start..span.end]
}

fn named(loaded: &Loaded, name: &str) -> Uuid {
    loaded
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("a node named {name}"))
        .id
}

fn at_path(loaded: &Loaded, path: &str) -> Uuid {
    loaded
        .nodes
        .iter()
        .find(|node| node.path == path)
        .unwrap_or_else(|| panic!("a node at {path}"))
        .id
}

fn link(graph: &Graph, node: Uuid, port: Uuid) -> &LinkSource {
    &graph
        .link_to(&Port::new(node, port))
        .unwrap_or_else(|| panic!("a link to {node}.{port}"))
        .source
}

/// The id of the node named, or at the path, `name`.
fn v5(name: &str) -> Uuid {
    Uuid::new_v5(
        &uuid::uuid!("cd5d3316-594e-459a-b1a5-6061fadc477e"),
        name.as_bytes(),
    )
}

// Parsing.
//================================================================

/// The example parses: the root and its tree by indentation, then the
/// detached nodes, each with its name, its target and its call.
#[test]
fn the_example_parses_into_a_tree_and_its_detached_nodes() {
    let document = parsed(EXAMPLE);
    assert_eq!(document.nodes.len(), 6, "the root and five detached nodes");
    let root = &document.nodes[0];
    assert_eq!(root.call.function.name.text, "fallback");
    assert!(root.call.arguments.is_empty());
    assert_eq!(root.comments[0].text, " comment to end of line");
    assert_eq!(
        root.comment.as_ref().unwrap().text,
        " the first top-level node is the root"
    );
    let sequence = &root.children[0];
    assert_eq!(sequence.children.len(), 4);
    assert_eq!(root.children[1].call.function.name.text, "succeed");

    let level = &document.nodes[3];
    assert_eq!(level.name.as_ref().unwrap().text, "level");
    assert_eq!(
        level.target.as_ref().unwrap().kind,
        ExprKind::Variable {
            name: "face/level".to_string()
        }
    );
    let sway = &document.nodes[5];
    let ExprKind::Call { call } = &sway.call.arguments[0].value.kind else {
        panic!("a nested call");
    };
    assert_eq!(call.function.name.text, "add");
    assert!(document.nodes[1].blank_before);
    assert!(!sway.blank_before);
}

/// Every name, argument and expression has the span of its text.
#[test]
fn every_part_has_the_span_of_its_text() {
    let text = "fallback\n  s: smooth(x: @raw.state, alpha: -0.5f32, state: {.s}[.v])\n";
    let document = parsed(text);
    let node = &document.nodes[0].children[0];
    assert_eq!(
        at(text, node.span),
        "s: smooth(x: @raw.state, alpha: -0.5f32, state: {.s}[.v])"
    );
    assert_eq!(at(text, node.name.as_ref().unwrap().span), "s");
    assert_eq!(
        at(text, node.call.span),
        "smooth(x: @raw.state, alpha: -0.5f32, state: {.s}[.v])"
    );
    let [x, alpha, state] = &node.call.arguments[..] else {
        panic!("three arguments");
    };
    assert_eq!(at(text, x.span), "x: @raw.state");
    assert_eq!(at(text, x.value.span), "@raw.state");
    let ExprKind::Port {
        node: read,
        parameter,
    } = &x.value.kind
    else {
        panic!("a port");
    };
    assert_eq!(at(text, read.span), "raw");
    assert_eq!(at(text, parameter.as_ref().unwrap().span), "state");
    assert_eq!(
        alpha.value.kind,
        ExprKind::Number {
            text: "-0.5".to_string(),
            suffix: Some("f32".to_string())
        }
    );
    assert_eq!(at(text, state.value.span), "{.s}[.v]");
    let ExprKind::Select { source, path } = &state.value.kind else {
        panic!("a selection");
    };
    assert_eq!(at(text, source.span), "{.s}");
    assert_eq!(path, ".v");
}

/// Every literal form the display writes reads back.
#[test]
fn every_literal_parses() {
    let text = "f(a: 1.5, b: 1.5f64, c: \"q\\\"\\n\\u{1b}\", d: true, e: (), f: f64[1, 2.5], \
                g: [1u8, \"a\"], h: some(none), i: uuid(5f726574-0000-4000-8000-000000000000), \
                j: json(\"{\\\"u8\\\":1}\"), k: {<x>}, l: NaNf64, m: -inff32, n: str[\"a\"])\n";
    let document = parsed(text);
    let kinds: Vec<&ExprKind> = document.nodes[0]
        .call
        .arguments
        .iter()
        .map(|argument| &argument.value.kind)
        .collect();
    assert_eq!(
        *kinds[2],
        ExprKind::String {
            value: "q\"\n\u{1b}".to_string()
        }
    );
    assert_eq!(*kinds[4], ExprKind::Unit);
    assert!(
        matches!(kinds[5], ExprKind::Array { element: Some(element), elements } if element == "f64" && elements.len() == 2)
    );
    assert!(matches!(kinds[6], ExprKind::Array { element: None, .. }));
    assert!(matches!(kinds[7], ExprKind::Some { value } if value.kind == ExprKind::None));
    assert!(matches!(kinds[8], ExprKind::Uuid { value } if *value == _RET_PARAM_ID));
    assert_eq!(
        *kinds[9],
        ExprKind::Json {
            text: "{\"u8\":1}".to_string()
        }
    );
    assert_eq!(
        *kinds[10],
        ExprKind::Variable {
            name: "<x>".to_string()
        }
    );
    assert!(
        matches!(kinds[11], ExprKind::Number { text, suffix } if text == "NaN" && suffix.as_deref() == Some("f64"))
    );
    assert!(matches!(kinds[12], ExprKind::Number { text, .. } if text == "-inf"));
}

/// A line that does not parse is reported where it fails and left out; the
/// lines around it are kept, and so are the children of a line that does
/// not parse, under the node above it. An argument that does not parse is
/// left out of a call that keeps the others.
#[test]
fn a_line_that_does_not_parse_leaves_the_others() {
    let text = "\
sequence
  below(x: {a}, limit: 1)
  ) oops
    succeed
  below(x: , limit: 2)
  fail
";
    let parsed = parse(text);
    let errors = errors(&parsed.diagnostics);
    assert_eq!(errors.len(), 2, "{:?}", parsed.diagnostics);
    assert_eq!(at(text, errors[0].span), ")");
    assert_eq!(at(text, errors[1].span), ",");
    let root = &parsed.document.nodes[0];
    let functions: Vec<&str> = root
        .children
        .iter()
        .map(|child| child.call.function.name.text.as_str())
        .collect();
    assert_eq!(functions, ["below", "succeed", "below", "fail"]);
    let names: Vec<&str> = root.children[2]
        .call
        .arguments
        .iter()
        .map(|argument| argument.name.text.as_str())
        .collect();
    assert_eq!(names, ["limit"], "the argument that parses is kept");
}

/// A bracket that closes nothing is reported, and the call or array goes
/// on past it.
#[test]
fn a_stray_bracket_is_passed() {
    for (text, stray) in [
        ("f(a: 1 ], b: 2)\n", "]"),
        ("f(a: [1 ) ], b: 2)\n", ")"),
        ("f(] a: 1)\n", "]"),
    ] {
        let parsed = parse(text);
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{text}: {:?}",
            parsed.diagnostics
        );
        assert_eq!(at(text, parsed.diagnostics[0].span), stray, "{text}");
        assert_eq!(parsed.document.nodes.len(), 1, "{text}");
    }
}

/// Whatever the text, parsing, formatting and resolving it end, and every
/// span they report is a range of the text.
#[test]
fn any_text_is_read_to_its_end() {
    let functions = index();
    for text in [
        "",
        "\n\n# only a comment",
        "\"unterminated",
        "{unterminated\nsucceed",
        "f(a: \"\\é\")",
        "été(à: 1)",
        "sequence\r\n  succeed\r\n",
        "  indented\nfirst",
        "a: b: c: d",
        "@x = @y = f",
        "f(a: g(b: h(c: [[[[1]]]], d: some(some(none)))))",
        "f(a: 1)[x]",
        ")))(((",
        "::",
        "f(a: uuid(nope), b: json(1), c: f64[1, \"a\"])",
        "sequence\n  succeed\n    fail\n      running\n   fallback\n",
    ] {
        let parsed = parse(text);
        for diagnostic in &parsed.diagnostics {
            assert!(
                text.get(diagnostic.span.start..diagnostic.span.end)
                    .is_some(),
                "{text:?}: {diagnostic:?}"
            );
        }
        let loaded = resolve(&parsed.document, &functions);
        for diagnostic in &loaded.diagnostics {
            assert!(
                text.get(diagnostic.span.start..diagnostic.span.end)
                    .is_some(),
                "{text:?}: {diagnostic:?}"
            );
        }
        let formatted = format(&parsed.document, &functions);
        assert_eq!(
            format(&parse(&formatted).document, &functions),
            formatted,
            "{text:?}"
        );
    }
}

/// An unclosed bracket ends at the next line back at its line's
/// indentation: that line parses on its own.
#[test]
fn an_unclosed_bracket_ends_with_its_line() {
    let text = "sequence\n  below(x: {a}, limit: 1\n  succeed\n";
    let parsed = parse(text);
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    assert_eq!(at(text, parsed.diagnostics[0].span), "(");
    assert_eq!(parsed.document.nodes[0].children.len(), 2);
    assert_eq!(
        parsed.document.nodes[0].children[0].call.arguments.len(),
        2,
        "the arguments before the end of the line are kept"
    );
}

/// A call broken over lines, as `{:#}` displays it, is one node.
#[test]
fn a_call_broken_over_lines_is_one_node() {
    let text = "{proximity} = below(\n    x: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64),\n    limit: 30f64,\n)\nsucceed\n";
    let document = parsed(text);
    assert_eq!(document.nodes.len(), 2);
    assert_eq!(document.nodes[0].call.arguments.len(), 2);
}

/// Indentation is spaces: a tab is an error. A line indented as no
/// enclosing block is an error.
#[test]
fn indentation_is_spaces_and_consistent() {
    let parsed = parse("sequence\n\tsucceed\n");
    assert_eq!(parsed.diagnostics.len(), 1);
    assert!(parsed.diagnostics[0].message.contains("tab"));

    let parsed = parse("sequence\n    succeed\n  fail\n");
    assert_eq!(parsed.diagnostics.len(), 1);
    assert!(parsed.diagnostics[0].message.contains("indented"));
}

/// The anonymous cell and a computed function, parameter or module are the
/// display's: a text is told so.
#[test]
fn display_only_forms_are_refused_with_a_reason() {
    for (text, refused, reason) in [
        ("f(a: {= 1u8})\n", "{= 1u8}", "anonymous cell"),
        ("(@n.p)(a: 1u8)\n", "(", "computed function"),
        ("f((@n.p): 1u8)\n", "(", "computed parameter"),
    ] {
        let parsed = parse(text);
        assert_eq!(parsed.diagnostics.len(), 1, "{text}");
        assert_eq!(at(text, parsed.diagnostics[0].span), refused);
        assert!(parsed.diagnostics[0].message.contains(reason), "{text}");
    }
}

// Formatting.
//================================================================

/// The formatter writes the canonical layout, whatever the text's: two
/// spaces per level, a node without arguments without parentheses, the
/// arguments in parameter order, one blank line after the tree. Literals stay
/// as written, and comments where they were. Formatting twice changes
/// nothing.
#[test]
fn formatting_is_canonical_and_idempotent() {
    let text = "\
# the root
fallback()
    below( limit : 30 , x:@cm )   # a check
    succeed
cm: clamp(lo: 0.0, x: mul(b: 100, a: {sensor/distance}))


# the level
{face/level} = add(b: 1.5f64, a: @cm)
# the end
";
    let functions = index();
    let formatted = format(&parsed(text), &functions);
    assert_eq!(
        formatted,
        "\
# the root
fallback
  below(x: @cm, limit: 30)  # a check
  succeed

cm: clamp(x: mul(a: {sensor/distance}, b: 100), lo: 0.0)

# the level
{face/level} = add(a: @cm, b: 1.5f64)

# the end
"
    );
    assert_eq!(format(&parsed(&formatted), &functions), formatted);
    let example = format(&parsed(EXAMPLE), &functions);
    assert_eq!(format(&parsed(&example), &functions), example);
}

// Resolution.
//================================================================

/// A node's id is the UUID v5 of its name, or of its path when it has none;
/// a node named by an id has that id.
#[test]
fn node_ids_derive_from_names_and_paths() {
    let text = "\
fallback
  sequence
    succeed
    check: below(x: mul(a: {x}, b: 2), limit: 1)

below(x: 1, limit: 2)
00000000-0000-4000-8000-0000000000aa: below(x: 1, limit: 2)
";
    let loaded = loaded(text);
    let paths: Vec<(&str, Option<&str>)> = loaded
        .nodes
        .iter()
        .map(|node| (node.path.as_str(), node.name.as_deref()))
        .collect();
    assert_eq!(
        paths,
        [
            ("root", None),
            ("root.0", None),
            ("root.0.0", None),
            ("root.0.1", Some("check")),
            ("root.0.1.x", None),
            ("detached.0", None),
            ("detached.1", Some("00000000-0000-4000-8000-0000000000aa")),
        ]
    );
    for node in &loaded.nodes {
        let expected = match node.name.as_deref() {
            Some(name) => Uuid::parse_str(name).unwrap_or_else(|_| v5(name)),
            None => v5(&node.path),
        };
        assert_eq!(node.id, expected, "{}", node.path);
        assert!(loaded.graph.nodes.contains_key(&node.id));
    }
    assert_eq!(loaded.graph.root, Some(v5("root")));
    let attached: Vec<bool> = loaded.nodes.iter().map(|node| node.attached).collect();
    assert_eq!(attached, [true, true, true, true, false, false, false]);
    let check = named(&loaded, "check");
    let nested = at_path(&loaded, "root.0.1.x");
    assert_eq!(
        loaded
            .nodes
            .iter()
            .find(|node| node.id == nested)
            .unwrap()
            .parent,
        Some(check),
        "a call's node is held by the node whose argument it is"
    );
    assert_eq!(
        loaded.graph.nodes[&at_path(&loaded, "root.0")].children,
        Some(vec![v5("root.0.0"), check])
    );
}

/// Variables are named by their key, ids derived as the graph lowering
/// derives them; a target binds `_ret`; `@n` reads a node's return value,
/// `@n.p` its parameter; a call in an argument is a node read by its return
/// value.
#[test]
fn references_resolve_to_variables_ports_and_nodes() {
    let loaded = loaded(EXAMPLE);
    let graph = &loaded.graph;
    let (raw, cm, level, s, sway) = (
        named(&loaded, "raw"),
        named(&loaded, "cm"),
        named(&loaded, "level"),
        named(&loaded, "s"),
        named(&loaded, "sway"),
    );
    for name in [
        "person/visible",
        ".mood",
        "sensor/distance",
        "face/level",
        "arora/time",
    ] {
        assert_eq!(
            graph
                .variables
                .get(&gen_uuid_from_str(name))
                .map(String::as_str),
            Some(name)
        );
    }
    assert_eq!(
        *link(graph, raw, MUL_A),
        LinkSource::Variable(gen_uuid_from_str("sensor/distance"))
    );
    assert_eq!(
        *link(graph, raw, MUL_B),
        LinkSource::Literal(Value::F64(100.0))
    );
    assert_eq!(
        *link(graph, cm, CLAMP_X),
        LinkSource::Port(Port::new(raw, _RET_PARAM_ID))
    );
    assert_eq!(
        *link(graph, level, _RET_PARAM_ID),
        LinkSource::Variable(gen_uuid_from_str("face/level"))
    );
    let reader = at_path(&loaded, "root.0.3");
    assert_eq!(
        *link(graph, reader, SMOOTH_STATE),
        LinkSource::Port(Port::new(s, SMOOTH_STATE))
    );
    let nested = at_path(&loaded, "detached.4.a");
    assert_eq!(
        *link(graph, sway, MUL_A),
        LinkSource::Port(Port::new(nested, _RET_PARAM_ID))
    );
    assert_eq!(
        *link(graph, nested, ADD_A),
        LinkSource::Variable(gen_uuid_from_str("arora/time"))
    );
    let set_string = at_path(&loaded, "root.0.1");
    assert_eq!(
        *link(graph, set_string, SET_STR_VALUE_PARAM_ID),
        LinkSource::Literal(Value::String("happy".to_string()))
    );
    assert_eq!(
        *link(graph, set_string, SET_STR_VARIABLE_PARAM_ID),
        LinkSource::Variable(gen_uuid_from_str(".mood"))
    );
    assert_eq!(
        graph.nodes[&at_path(&loaded, "root.0.0")].function,
        EQUAL_FUNCTION_ID
    );
    assert_eq!(graph.nodes[&v5("root")].function, FALLBACK_FUNCTION_ID);
    assert_eq!(
        graph.nodes[&at_path(&loaded, "root.0")].function,
        SEQ_FUNCTION_ID
    );
    let smooth = &graph.nodes[&s];
    assert_eq!(
        smooth.outputs.iter().map(|io| io.id).collect::<Vec<_>>(),
        [_RET_PARAM_ID, SMOOTH_STATE],
        "the return value and the mutable parameter are outputs"
    );
    assert_eq!(
        smooth.inputs[0].ty,
        Some(arora_types::module::high::TypeRef::Scalar {
            id: "f64".to_string()
        })
    );
}

/// A literal takes its parameter's type; one that states its type keeps it,
/// and must match; an optional parameter takes its content's type.
#[test]
fn literals_take_the_type_of_their_parameter() {
    let loaded = loaded("clamp(x: 1, lo: -2.5f64, hi: 7)\n");
    let root = v5("root");
    assert_eq!(
        *link(&loaded.graph, root, CLAMP_X),
        LinkSource::Literal(Value::F64(1.0))
    );
    assert_eq!(
        *link(&loaded.graph, root, CLAMP_LO),
        LinkSource::Literal(Value::F64(-2.5))
    );
    assert_eq!(
        *link(&loaded.graph, root, CLAMP_HI),
        LinkSource::Literal(Value::F64(7.0))
    );

    let text = "sequence\n  clamp(x: 1u8, lo: \"a\")\n  equal(a: {k}, b: 3)\n";
    let loaded = resolve(&parsed(text), &index());
    let errors = errors(&loaded.diagnostics);
    let refused: Vec<&str> = errors.iter().map(|error| at(text, error.span)).collect();
    assert_eq!(refused, ["1u8", "\"a\"", "3"], "{:?}", loaded.diagnostics);
    assert!(
        errors[2].message.contains("`3f64`"),
        "{}",
        errors[2].message
    );
}

/// A function or parameter that is not found is an error at its name; a
/// bare name two modules have is one too, and `module::name` picks one.
#[test]
fn unknown_and_ambiguous_names_are_errors_at_the_name() {
    let text =
        "sequence\n  nope(x: 1)\n  below(y: 1, x: 2, limit: 3)\n  below(x: @nobody, limit: 1)\n";
    let loaded = resolve(&parsed(text), &index());
    let refused: Vec<&str> = errors(&loaded.diagnostics)
        .iter()
        .map(|error| at(text, error.span))
        .collect();
    assert_eq!(refused, ["nope", "y", "nobody"]);

    let mut functions = index();
    let (id, twin) = function(
        Uuid::from_u128(0xAB00),
        Uuid::from_u128(0xAB01),
        "mul",
        vec![(Uuid::from_u128(0xAB11), parameter("a", f64_ty(), false))],
    );
    functions.insert(id, twin);
    let text = "mul(a: 1, b: 2)\n";
    let ambiguous = resolve(&parsed(text), &functions);
    assert_eq!(errors(&ambiguous.diagnostics).len(), 1);
    assert_eq!(at(text, ambiguous.diagnostics[0].span), "mul");
    let picked = loaded_with(&format!("{MATH}::mul(a: 1, b: 2)\n"), &functions);
    assert_eq!(picked.graph.nodes[&v5("root")].function, MUL);
}

/// A native's own state is seeded when a node leaves it out; a node that
/// leaves out what its function needs, and a node nothing holds that is not
/// a module call, are warned about.
#[test]
fn natives_are_seeded_and_omissions_warned() {
    let text = "sequence_star\n  below(x: 1)\n\nfallback\n  succeed\n";
    let loaded = resolve(&parsed(text), &index());
    assert_eq!(
        *link(&loaded.graph, v5("root"), SEQ_STAR_CURRENT_INDEX_PARAM_ID),
        LinkSource::Literal(Value::U16(0))
    );
    let warnings: Vec<&str> = loaded
        .diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.severity, Severity::Warning);
            at(text, diagnostic.span)
        })
        .collect();
    assert_eq!(warnings, ["below(x: 1)", "fallback"]);
}

/// The natives list their names, parameters and types, for a palette.
#[test]
fn natives_describe_their_signatures() {
    let names: Vec<&str> = natives()
        .map(|native| native.function_name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "sequence",
            "sequence_star",
            "fallback",
            "parallel",
            "succeed",
            "fail",
            "running",
            "equal",
            "unpack",
            "scatter",
            "set_string",
            "unset_string",
            "is_string_set",
            "wait_string_set",
            "regex_match",
            "run_call",
            "run_status",
        ]
    );
    let set_string = natives()
        .find(|native| native.function_name == "set_string")
        .unwrap();
    let parameters: Vec<(&str, bool)> = set_string
        .function
        .parameter_ordering
        .iter()
        .map(|id| {
            let parameter = &set_string.function.parameters[id];
            (parameter.name.as_str(), parameter.mutable)
        })
        .collect();
    assert_eq!(parameters, [("variable", true), ("value", false)]);
    assert!(natives().all(|native| native.module_id.is_nil()));
}

/// The AST and what resolution gives are serde values, for the editor.
#[test]
fn the_ast_and_the_loaded_graph_are_serde_values() {
    let document = parsed(EXAMPLE);
    let json = serde_json::to_string(&document).unwrap();
    assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), document);
    let loaded = loaded(EXAMPLE);
    let json = serde_json::to_string(&loaded).unwrap();
    assert_eq!(serde_json::from_str::<Loaded>(&json).unwrap(), loaded);
}

// The display, read back.
//================================================================

/// `graph` with its links in one order, to compare graphs whose links were
/// added in different orders.
fn sorted(mut graph: Graph) -> Graph {
    graph
        .links
        .sort_by_key(|link| (link.target.node, link.target.port));
    graph
}

/// A tree shown as text reads back to the graph it was built from: every
/// node with its id, every link, every variable.
#[test]
fn a_displayed_tree_reads_back_to_its_graph() {
    let functions = index();
    let loaded = loaded(EXAMPLE);
    let tree = build_behavior_tree(&loaded.graph, &|_| None).expect("the tree builds");
    let node_names: HashMap<Uuid, String> = loaded
        .nodes
        .iter()
        .filter_map(|node| Some((node.id, node.name.clone()?)))
        .collect();
    let names = Names {
        functions: Some(&functions),
        variables: Some(&loaded.graph.variables),
        nodes: Some(&node_names),
    };
    let shown = tree.display(names).to_string();
    let read = resolve(&parsed(&shown), &functions);
    assert_eq!(
        errors(&read.diagnostics),
        Vec::<&Diagnostic>::new(),
        "{shown}"
    );
    assert_eq!(
        sorted(read.graph.clone()),
        sorted(loaded.graph.clone()),
        "{shown}"
    );
    // Read back, it shows the same.
    let again = build_behavior_tree(&read.graph, &|_| None).unwrap();
    assert_eq!(again.display(names).to_string(), shown);

    // Without names, nodes are named by their ids, and read back the same.
    let anonymous = tree.display(Names::functions(&functions)).to_string();
    let read = resolve(&parsed(&anonymous), &functions);
    assert_eq!(
        errors(&read.diagnostics),
        Vec::<&Diagnostic>::new(),
        "{anonymous}"
    );
    let mut expected = sorted(loaded.graph.clone());
    // Variables are shown by their ids, which name none.
    expected.variables.clear();
    assert_eq!(sorted(read.graph), expected, "{anonymous}");
}

/// The display of a tree built from nodes, a nested call among their
/// arguments, reads back: each call a node of its own.
#[test]
fn a_tree_built_from_nodes_displays_as_text() {
    let tree = load_behavior_tree_nodes(vec![node(
        1,
        BELOW,
        vec![(BELOW_X, centimetres()), (BELOW_LIMIT, f(30.0))],
    )])
    .unwrap();
    let functions = index();
    let shown = tree.display(Names::functions(&functions)).to_string();
    assert_eq!(
        shown,
        "below(x: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64), limit: 30f64)\n"
    );
    let read = resolve(&parsed(&shown), &functions);
    assert_eq!(errors(&read.diagnostics), Vec::<&Diagnostic>::new());
    assert_eq!(read.graph.nodes.len(), 4, "the leaf and three calls");
}

// Texts run.
//================================================================

fn store_with(values: &[(&str, f64)]) -> SimpleDataStore {
    let store = SimpleDataStore::new();
    for (key, value) in values {
        set(&store, key, *value);
    }
    store
}

fn set(store: &SimpleDataStore, key: &str, value: f64) {
    store
        .write(StateChange::set(key, Value::F64(value)))
        .expect("the store writes");
}

fn get(store: &SimpleDataStore, key: &str) -> Option<Value> {
    store.read(&[Key::from(key)]).remove(0)
}

/// Load `text` over `store`.
fn tree(store: &SimpleDataStore, text: &str) -> BehaviorTree {
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    load_behavior_tree_text(text, &index(), &resolver).expect("the text loads")
}

/// How many calls each function received since the last time this was asked.
fn take(calls: &Calls) -> HashMap<&'static str, usize> {
    let mut counts = HashMap::new();
    for (name, _) in calls.borrow_mut().drain(..) {
        *counts.entry(name).or_insert(0) += 1;
    }
    counts
}

/// The centimetres cascade read by three conditions under a parallel: the
/// cascade is called once and the three conditions read it. Written in each
/// condition, it is three cascades, each called once; settled, none is
/// called again.
#[test]
fn a_cascade_read_by_three_conditions_runs_once() {
    let store = store_with(&[("sensor/distance", 0.2)]);
    let text = "\
parallel
  below(x: @cm, limit: 10)
  below(x: @cm, limit: 30)
  below(x: @cm, limit: 50)

raw: mul(a: {sensor/distance}, b: 100)
cm: clamp(x: @raw, lo: 0)
";
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(
        runtime.tick().unwrap(),
        Status::Failure,
        "20 cm is not below 10"
    );
    let counts = take(&calls);
    assert_eq!((counts["mul"], counts["clamp"], counts["below"]), (1, 1, 3));
    assert_eq!(tree.detached_calls(v5("cm")), Some(1));

    let inline = "\
parallel
  below(x: clamp(x: mul(a: {sensor/distance}, b: 100), lo: 0), limit: 10)
  below(x: clamp(x: mul(a: {sensor/distance}, b: 100), lo: 0), limit: 30)
  below(x: clamp(x: mul(a: {sensor/distance}, b: 100), lo: 0), limit: 50)
";
    let tree = self::tree(&store, inline);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    let counts = take(&calls);
    assert_eq!((counts["mul"], counts["clamp"], counts["below"]), (3, 3, 3));
    runtime.tick().unwrap();
    assert_eq!(take(&calls), HashMap::from([("below", 3)]), "settled");
}

/// Under a fallback, a condition that does not tick reads nothing: the node
/// it alone reads is not called, and a node nothing reads never is.
#[test]
fn under_a_fallback_only_what_a_ticking_condition_reads_runs() {
    let store = store_with(&[("left", 1.0), ("right", 1.0)]);
    let text = "\
fallback
  below(x: @l, limit: 100)
  below(x: @r, limit: 100)

l: mul(a: {left}, b: 2)
r: mul(a: {right}, b: 3)
unused: mul(a: {left}, b: 9)
";
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    let mut on_demand = vec![v5("l"), v5("r")];
    on_demand.sort();
    let mut plan = tree.detached_plan();
    plan.on_demand.sort();
    assert_eq!(plan.on_demand, on_demand);
    assert_eq!(plan.unused, [v5("unused")]);
    assert_eq!(runtime.tick().unwrap(), Status::Success);
    assert_eq!(tree.detached_calls(v5("l")), Some(1));
    assert_eq!(
        tree.detached_calls(v5("r")),
        Some(0),
        "the second child did not tick"
    );
    assert_eq!(tree.detached_calls(v5("unused")), Some(0));
}

/// A sink, a node whose return value is bound to a store key, runs every
/// tick though the tree does not read it, writing its key from its cache
/// when its inputs did not change. A node writing a tree-local variable that
/// nothing reads never runs.
#[test]
fn a_sink_writes_its_key_every_tick_from_its_cache() {
    let store = store_with(&[("in", 3.0)]);
    let text = "\
below(x: 1, limit: 2)

double: {out} = mul(a: {in}, b: 2)
local: {.x} = mul(a: {in}, b: 5)
";
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(tree.detached_plan().every_tick, [v5("double")]);
    assert_eq!(tree.detached_plan().unused, [v5("local")]);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));

    set(&store, "out", -1.0);
    runtime.tick().unwrap();
    assert_eq!(
        get(&store, "out"),
        Some(Value::F64(6.0)),
        "written back from the cache"
    );
    assert_eq!(tree.detached_calls(v5("double")), Some(1));
    assert_eq!(tree.detached_calls(v5("local")), Some(0));

    set(&store, "in", 4.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(8.0)));
    assert_eq!(tree.detached_calls(v5("double")), Some(2));
}

/// A stateful node keeps its state in its mutable parameter, from the
/// literal the text gives it, and is called until the state settles.
#[test]
fn a_stateful_node_settles() {
    let store = store_with(&[("target", 2.0)]);
    let text = "\
below(x: @s.state, limit: 100)

s: {out} = smooth(x: {target}, alpha: 1, state: 0)
";
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    let mut smooth_calls = Vec::new();
    for _ in 0..4 {
        runtime.tick().unwrap();
        smooth_calls.push(take(&calls).get("smooth").copied().unwrap_or(0));
    }
    assert_eq!(smooth_calls, [1, 1, 0, 0]);
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));
}

/// A text with an error does not load, and says where.
#[test]
fn a_text_with_an_error_does_not_load() {
    let text = "sequence\n  below(x: @nobody, limit: 1)\n";
    let error = match load_behavior_tree_text(text, &index(), &|_| None) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("the text does not load"),
    };
    assert!(error.contains("2:13: no node is named `nobody`"), "{error}");
}
