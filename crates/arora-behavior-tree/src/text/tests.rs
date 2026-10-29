//! The text language: parsing and its recovery, formatting, resolution, the
//! display read back, and texts run end to end on the fake engine of
//! [`call_expression_tests`](crate::call_expression_tests).

use std::collections::HashMap;

use arora_behavior::graph::{Graph, Port, Source};
use arora_behavior::{FunctionIndex, Status};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
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
    below(x: @cm._ret, limit: 30)
    smooth(x: @raw._ret, alpha: 0.2, state: @s.state)
  succeed

raw: mul(a: {sensor/distance}, b: 100)
cm: clamp(x: @raw._ret, lo: 0)
level: {face/level} = add(a: @cm._ret, b: 1)
s: smooth(x: @cm._ret, alpha: 0.5, state: 0)
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

fn link(graph: &Graph, node: Uuid, port: Uuid) -> &Source {
    graph
        .node(&node)
        .and_then(|node| node.bind.get(&port))
        .unwrap_or_else(|| panic!("a binding of {node}.{port}"))
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
    assert_eq!(at(text, parameter.span), "state");
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
        "@x._ret = @y._ret = f",
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
        match format(&parsed, &functions) {
            Ok(formatted) => assert_eq!(
                format(&parse(&formatted), &functions),
                Ok(formatted),
                "{text:?}"
            ),
            Err(errors) => assert_eq!(
                errors,
                self::errors(&parsed.diagnostics)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            ),
        }
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
    below( limit : 30 , x:@cm._ret )   # a check
    succeed
cm: clamp(lo: 0.0, x: mul(b: 100, a: {sensor/distance}))


# the level
{face/level} = add(b: 1.5f64, a: @cm._ret)
# the end
";
    let functions = index();
    let formatted = format(&parse(text), &functions).unwrap();
    assert_eq!(
        formatted,
        "\
# the root
fallback
  below(x: @cm._ret, limit: 30)  # a check
  succeed

cm: clamp(x: mul(a: {sensor/distance}, b: 100), lo: 0.0)

# the level
{face/level} = add(a: @cm._ret, b: 1.5f64)

# the end
"
    );
    assert_eq!(format(&parse(&formatted), &functions), Ok(formatted));
    let example = format(&parse(EXAMPLE), &functions).unwrap();
    assert_eq!(format(&parse(&example), &functions), Ok(example));
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
        assert!(loaded.graph.node(&node.id).is_some());
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
        loaded
            .graph
            .node(&at_path(&loaded, "root.0"))
            .unwrap()
            .children,
        Some(vec![v5("root.0.0"), check])
    );
}

/// A bare `@node` names no port: it is refused where it is written, with the
/// return value's spelling, `@node._ret`, in the message.
#[test]
fn a_bare_node_reference_is_refused_with_its_return_value_s_spelling() {
    for (text, reference) in [
        (
            "below(x: @cm, limit: 30)\n\ncm: clamp(x: 1, lo: 0)\n",
            "@cm",
        ),
        ("@cm = add(a: 1, b: 2)\n", "@cm"),
        ("below(x: @cm[.0], limit: 30)\n", "@cm"),
    ] {
        let parsed = parse(text);
        let [diagnostic] = parsed.diagnostics.as_slice() else {
            panic!("one error: {:?}", parsed.diagnostics);
        };
        assert_eq!(diagnostic.severity, Severity::Error, "{text}");
        assert_eq!(at(text, diagnostic.span), reference, "{text}");
        assert!(
            diagnostic.message.contains("`@cm._ret`"),
            "{text}: {}",
            diagnostic.message
        );
    }
}

/// A variable binds its key; a target binds `_ret`; `@n._ret` reads a node's
/// return value, `@n.p` its parameter; a call in an argument is a node read
/// by its return value.
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
    let keys: std::collections::BTreeSet<&str> = graph
        .nodes
        .iter()
        .flat_map(|node| node.bind.values())
        .filter_map(|source| match source {
            Source::Key(key) => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        [
            "person/visible",
            ".mood",
            "sensor/distance",
            "face/level",
            "arora/time",
            "rig/arm_wave",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        *link(graph, raw, MUL_A),
        Source::Key("sensor/distance".to_string())
    );
    assert_eq!(*link(graph, raw, MUL_B), Source::Value(Value::F64(100.0)));
    assert_eq!(
        *link(graph, cm, CLAMP_X),
        Source::Port(Port::new(raw, _RET_PARAM_ID))
    );
    assert_eq!(
        *link(graph, level, _RET_PARAM_ID),
        Source::Key("face/level".to_string())
    );
    let reader = at_path(&loaded, "root.0.3");
    assert_eq!(
        *link(graph, reader, SMOOTH_STATE),
        Source::Port(Port::new(s, SMOOTH_STATE))
    );
    let nested = at_path(&loaded, "detached.4.a");
    assert_eq!(
        *link(graph, sway, MUL_A),
        Source::Port(Port::new(nested, _RET_PARAM_ID))
    );
    assert_eq!(
        *link(graph, nested, ADD_A),
        Source::Key("arora/time".to_string())
    );
    let set_string = at_path(&loaded, "root.0.1");
    assert_eq!(
        *link(graph, set_string, SET_STR_VALUE_PARAM_ID),
        Source::Value(Value::String("happy".to_string()))
    );
    assert_eq!(
        *link(graph, set_string, SET_STR_VARIABLE_PARAM_ID),
        Source::Key(".mood".to_string())
    );
    assert_eq!(
        graph.node(&at_path(&loaded, "root.0.0")).unwrap().function,
        EQUAL_FUNCTION_ID
    );
    assert_eq!(
        graph.node(&v5("root")).unwrap().function,
        FALLBACK_FUNCTION_ID
    );
    assert_eq!(
        graph.node(&at_path(&loaded, "root.0")).unwrap().function,
        SEQ_FUNCTION_ID
    );
}

/// A variable known by its id alone names no key: it binds the tree-local
/// key `.<uuid>`, which no store sees.
#[test]
fn a_variable_known_by_its_id_binds_a_tree_local_key() {
    let id = "00000000-0000-4000-8000-0000000000ab";
    let loaded = loaded(&format!("set_string(variable: {{{id}}}, value: \"x\")\n"));
    assert_eq!(
        *link(&loaded.graph, v5("root"), SET_STR_VARIABLE_PARAM_ID),
        Source::Key(format!(".{id}"))
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
        Source::Value(Value::F64(1.0))
    );
    assert_eq!(
        *link(&loaded.graph, root, CLAMP_LO),
        Source::Value(Value::F64(-2.5))
    );
    assert_eq!(
        *link(&loaded.graph, root, CLAMP_HI),
        Source::Value(Value::F64(7.0))
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
        "sequence\n  nope(x: 1)\n  below(y: 1, x: 2, limit: 3)\n  below(x: @nobody._ret, limit: 1)\n";
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
    assert_eq!(picked.graph.node(&v5("root")).unwrap().function, MUL);
}

/// A native's own state is seeded when a node leaves it out; a node that
/// leaves out what its function needs, and a node nothing holds that is not
/// a module call, are warned about.
#[test]
fn natives_are_seeded_and_omissions_warned() {
    let text = "sequence_star\n  equal(a: 1u8)\n  below(x: 1)\n\nfallback\n  succeed\n";
    let loaded = resolve(&parsed(text), &index());
    assert_eq!(
        *link(&loaded.graph, v5("root"), SEQ_STAR_CURRENT_INDEX_PARAM_ID),
        Source::Value(Value::U16(0))
    );
    let warnings: Vec<&str> = loaded
        .diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.severity, Severity::Warning);
            at(text, diagnostic.span)
        })
        .collect();
    // A module function's unstated parameter is passed `()` or `None`,
    // which it may mean: no warning.
    assert_eq!(warnings, ["equal(a: 1u8)", "fallback"]);
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

/// `graph` with its nodes in one order, to compare graphs whose nodes are
/// listed in different orders.
fn sorted(mut graph: Graph) -> Graph {
    graph.nodes.sort_by_key(|node| node.id);
    graph
}

/// A tree shown as text reads back to the graph it was built from: every
/// node with its id, every binding.
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
        variables: None,
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

    // Without names, nodes are named by their ids, and read back the same;
    // a tree built from a graph names its variables by their keys.
    let anonymous = tree.display(Names::functions(&functions)).to_string();
    let read = resolve(&parsed(&anonymous), &functions);
    assert_eq!(
        errors(&read.diagnostics),
        Vec::<&Diagnostic>::new(),
        "{anonymous}"
    );
    assert_eq!(
        sorted(read.graph),
        sorted(loaded.graph.clone()),
        "{anonymous}"
    );
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
  below(x: @cm._ret, limit: 10)
  below(x: @cm._ret, limit: 30)
  below(x: @cm._ret, limit: 50)

raw: mul(a: {sensor/distance}, b: 100)
cm: clamp(x: @raw._ret, lo: 0)
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
  below(x: @l._ret, limit: 100)
  below(x: @r._ret, limit: 100)

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
    let text = "sequence\n  below(x: @nobody._ret, limit: 1)\n";
    let error = match load_behavior_tree_text(text, &index(), &|_| None) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("the text does not load"),
    };
    assert!(error.contains("2:13: no node is named `nobody`"), "{error}");
}

// Bounds and edge cases.
//================================================================

/// Run `read` on a thread with a small stack: 256 KiB, a quarter of a wasm
/// module's default, in a release build, which the editor's wasm is. A debug
/// build's frames are about five times a release build's, and its stack is
/// sized alike.
fn on_a_small_stack(read: impl FnOnce() + Send + 'static) {
    let stack = if cfg!(debug_assertions) { 5 } else { 1 } * 256 * 1024;
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(read)
        .expect("a thread")
        .join()
        .expect("no stack overflow");
}

/// Whether `diagnostics` report a nesting past [`MAX_DEPTH`].
fn too_deep(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("nest at most"))
}

/// Expressions and blocks nested past the limit are reported, and parsing,
/// resolving and formatting stay within a small stack.
#[test]
fn nesting_past_the_limit_is_reported_on_a_small_stack() {
    let past = MAX_DEPTH + 1;
    let texts = [
        format!("f(a: {}1{})\n", "some(".repeat(past), ")".repeat(past)),
        format!("f(a: {}1{})\n", "[".repeat(past), "]".repeat(past)),
        format!("{}1{}\n", "f(a: ".repeat(past), ")".repeat(past)),
        format!("f(a: {{x}}{})\n", "[a]".repeat(20_000)),
        (0..3_000)
            .map(|depth| format!("{}sequence\n", " ".repeat(depth)))
            .collect::<String>(),
    ];
    for text in texts {
        on_a_small_stack(move || {
            let parsed = parse(&text);
            assert!(too_deep(&parsed.diagnostics), "{:?}", parsed.diagnostics);
            let span = parsed
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.message.contains("nest at most"))
                .unwrap()
                .span;
            assert!(text.get(span.start..span.end).is_some());
            resolve(&parsed.document, &index());
            assert!(format(&parsed, &index()).is_err());
        });
    }
}

/// A document built deeper than the limit, not by the parser, is refused by
/// resolution and formatting alike.
#[test]
fn a_document_built_too_deep_is_refused() {
    on_a_small_stack(|| {
        let mut value = Expr {
            span: Span::default(),
            kind: ExprKind::None,
        };
        for _ in 0..300 {
            value = Expr {
                span: Span::default(),
                kind: ExprKind::Some {
                    value: Box::new(value),
                },
            };
        }
        let mut document = parsed("equal(a: none, b: true)\n");
        document.nodes[0].call.arguments[0].value = value;
        assert!(too_deep(&resolve(&document, &index()).diagnostics));
        let parsed = Parsed {
            document,
            diagnostics: Vec::new(),
        };
        assert!(too_deep(&format(&parsed, &index()).unwrap_err()));
    });
}

/// A text with errors is not formatted: its document does not hold what did
/// not parse.
#[test]
fn a_text_with_errors_is_not_formatted() {
    let text = "sequence\n  ) oops\n    succeed\n  below(x: , limit: 2)\n";
    let parsed = parse(text);
    assert_eq!(
        format(&parsed, &index()),
        Err(errors(&parsed.diagnostics).into_iter().cloned().collect())
    );
}

/// A selection's path runs to the first `]` on its line, whatever it holds;
/// one with no `]` is reported.
#[test]
fn a_path_holds_any_character_up_to_its_bracket() {
    let text = "f(\n    a: {x}[a#b(c\"d],\n    b: @n.p[(],\n)\n";
    let document = parsed(text);
    let arguments = &document.nodes[0].call.arguments;
    assert_eq!(arguments.len(), 2);
    let paths: Vec<&str> = arguments
        .iter()
        .map(|argument| match &argument.value.kind {
            ExprKind::Select { path, .. } => path.as_str(),
            other => panic!("a selection, not {other:?}"),
        })
        .collect();
    assert_eq!(paths, ["a#b(c\"d", "("]);

    let parsed = parse("f(a: {x}[a)\nsucceed\n");
    assert!(parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("ends with `]`")));
    assert_eq!(parsed.document.nodes.len(), 2);
}

/// Comments inside a call broken over lines stay after the arguments they
/// follow.
#[test]
fn comments_in_a_broken_call_stay_with_their_arguments() {
    let text = "below(\n    limit: 2,  # the limit\n    # read from the store\n    x: {d},\n)\n";
    let formatted = format(&parse(text), &index()).unwrap();
    assert_eq!(
        formatted,
        "below(\n    x: {d},\n    limit: 2,  # the limit\n    # read from the store\n)\n"
    );
    assert_eq!(format(&parse(&formatted), &index()), Ok(formatted.clone()));
}

/// A byte order mark is no part of the text, and a lone `\r` ends a line.
#[test]
fn a_byte_order_mark_and_a_carriage_return_are_read() {
    for text in ["\u{FEFF}sequence\n  succeed\n", "sequence\r  succeed\r"] {
        let document = parsed(text);
        assert_eq!(document.nodes.len(), 1, "{text:?}");
        assert_eq!(document.nodes[0].call.function.name.text, "sequence");
        assert_eq!(document.nodes[0].children.len(), 1);
    }
}

/// A finite number past its type's range is refused, not made infinite.
#[test]
fn a_number_past_its_range_is_refused() {
    for (text, refused) in [
        ("clamp(x: 1e999f64, lo: 0)\n", "1e999f64"),
        ("clamp(x: 1e999, lo: 0)\n", "1e999"),
    ] {
        let loaded = resolve(&parsed(text), &index());
        let errors = errors(&loaded.diagnostics);
        assert_eq!(errors.len(), 1, "{text}");
        assert_eq!(at(text, errors[0].span), refused);
        assert!(errors[0].message.contains("range"));
    }
    let loaded = loaded("clamp(x: 1e300, lo: -inf)\n");
    assert_eq!(
        *link(&loaded.graph, v5("root"), CLAMP_X),
        Source::Value(Value::F64(1e300))
    );
    assert_eq!(
        crate::display::literal(&Value::F64(1e300)),
        "1e300f64",
        "shown in its shortest form"
    );
}

/// `\u{…}` holds hex digits only.
#[test]
fn a_unicode_escape_holds_hex_digits() {
    let parsed = parse("f(a: \"\\u{+41}\")\n");
    assert_eq!(parsed.diagnostics.len(), 1);
    assert!(parsed.diagnostics[0].message.contains("\\u{…}"));
}

/// Two nodes writing one store key are warned about, each at its line, with
/// the one that wins; running the text, that one's value is the key's.
#[test]
fn two_nodes_writing_one_key_are_warned_about() {
    let text = "succeed\n\na: {out} = mul(a: 1, b: 2)\nb: {out} = mul(a: 1, b: 3)\n";
    let loaded = resolve(&parsed(text), &index());
    let warnings: Vec<(&str, &str)> = loaded
        .diagnostics
        .iter()
        .map(|diagnostic| (at(text, diagnostic.span), diagnostic.message.as_str()))
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let (winner, value) = if v5("a") > v5("b") {
        ("a", 2.0)
    } else {
        ("b", 3.0)
    };
    for (line, message) in &warnings {
        if line.starts_with(winner) {
            assert!(message.contains("this node publishes it last"), "{message}");
        } else {
            assert!(
                message.contains(&format!("`{winner}` publishes it last")),
                "{message}"
            );
        }
    }
    let store = store_with(&[]);
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(value)));
}

/// `@n.p = f(…)` writes the return value into `n`'s input `p`, which then
/// reads the node: the reader ticks with it.
#[test]
fn a_return_value_written_to_an_input_feeds_it() {
    let text = "r: below(x: 1000, limit: 30)\n\nfeed: @r.x = mul(a: {in}, b: 2)\n";
    let loaded = loaded(text);
    assert_eq!(
        *link(&loaded.graph, v5("feed"), _RET_PARAM_ID),
        Source::Port(Port::new(v5("r"), BELOW_X))
    );
    let store = store_with(&[("in", 10.0)]);
    let tree = tree(&store, text);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(runtime.tick().unwrap(), Status::Success, "20 is below 30");
    let read = calls
        .borrow()
        .iter()
        .find(|(name, _)| *name == "below")
        .map(|(_, args)| args[&BELOW_X].clone());
    assert_eq!(read, Some(Value::F64(20.0)));
}

/// What lowering refuses is an error where it is written: a reference to no
/// node, a detached node's port that is no output, a cycle of detached
/// nodes, a node reading its own output. A tree node's argument is read.
#[test]
fn what_lowering_refuses_is_an_error_where_written() {
    for (text, refused) in [
        (
            "below(x: @00000000-0000-4000-8000-0000000000ee._ret, limit: 1)\n",
            vec!["00000000-0000-4000-8000-0000000000ee"],
        ),
        (
            "below(x: @m.a, limit: 1)\n\nm: mul(a: 1, b: 2)\n",
            vec!["a"],
        ),
        (
            "below(x: @a._ret, limit: 1)\n\na: add(a: @b._ret, b: 1)\nb: add(a: @a._ret, b: 1)\n",
            vec!["a: add(a: @b._ret, b: 1)", "b: add(a: @a._ret, b: 1)"],
        ),
        (
            "succeed\n\ne: add(a: @e._ret, b: 1)\n",
            vec!["e: add(a: @e._ret, b: 1)"],
        ),
    ] {
        let loaded = resolve(&parsed(text), &index());
        let errors: Vec<&str> = errors(&loaded.diagnostics)
            .iter()
            .map(|error| at(text, error.span))
            .collect();
        assert_eq!(errors, refused, "{text}: {:?}", loaded.diagnostics);
    }
    loaded("sequence\n  t: below(x: 1, limit: 4)\n  below(x: @m._ret, limit: 100)\n\nm: mul(a: @t.limit, b: 10)\n");
    loaded("below(x: @s.state, limit: 1)\n\ns: smooth(x: 1, alpha: 1, state: 0)\n");
}

/// A name given twice is reported at both nodes.
#[test]
fn a_name_given_twice_is_reported_at_both() {
    let text = "sequence\n  n: succeed\n  n: fail\n";
    let loaded = resolve(&parsed(text), &index());
    let mut names: Vec<usize> = errors(&loaded.diagnostics)
        .iter()
        .map(|error| error.span.start)
        .collect();
    names.sort();
    assert_eq!(names, [11, 24]);
}

/// A call in `some(…)` is told that `some(…)` holds literals.
#[test]
fn some_holds_literals() {
    let text = "clamp(x: 1, lo: 0, hi: some(mul(a: 1, b: 2)))\n";
    let loaded = resolve(&parsed(text), &index());
    assert!(errors(&loaded.diagnostics)[0].message.contains("`some(…)`"));
}

/// An error's column counts characters.
#[test]
fn an_error_column_counts_characters() {
    let text = "sequence\n  set_string(variable: {\"é\"}, value: @nobody._ret)\n";
    let error = match load_behavior_tree_text(text, &index(), &|_| None) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("the text does not load"),
    };
    let column = text.lines().nth(1).unwrap().find("nobody").unwrap();
    let column = text.lines().nth(1).unwrap()[..column].chars().count() + 1;
    assert!(
        error.contains(&format!("2:{column}: no node is named")),
        "{error}"
    );
}

/// Whatever the keys, node names, function and parameter names, a tree's
/// display reads back to its graph: a key is quoted when it needs it, and a
/// name that is not written as a name as it is shows as the id.
#[test]
fn a_display_reads_back_whatever_the_names() {
    use arora_behavior::graph::Node as GraphNode;
    let keys = [
        "a}b",
        "a b",
        "=x",
        "line\nbreak",
        "x # y",
        "",
        "\"q\"",
        "é/ü",
        "{x}",
        "tab\there",
        ".local key",
        "00000000-0000-4000-8000-0000000000ab",
        "plain/key",
        "a\\b",
    ];
    let names = [
        "my node",
        "a:b",
        "x-y",
        "équipe",
        "inf",
        "NaN",
        "true",
        "some",
        "ok_name",
        "00000000-0000-4000-8000-0000000000cd",
        "fine",
    ];
    let (first, second, other) = (
        Uuid::from_u128(0xAA00),
        Uuid::from_u128(0xBB00),
        Uuid::from_u128(0xCC00),
    );
    let mut functions = index();
    let mut add = |module: Uuid, id: u128, name: &str, parameter_name: &str| {
        let parameter_id = Uuid::from_u128(id + 1);
        let (id, function) = function(
            module,
            Uuid::from_u128(id),
            name,
            vec![(parameter_id, parameter(parameter_name, f64_ty(), false))],
        );
        functions.insert(id, function);
        (id, parameter_id)
    };
    let (shadowed, shadowed_p) = add(first, 0xAA01, "sequence", "inf");
    let (spaced, spaced_p) = add(second, 0xBB01, "my fn", "a b");
    let (twin, twin_p) = add(other, 0xCC01, "mul", "a");

    for (index, key) in keys.iter().enumerate() {
        let name = |offset: usize| names[(index + offset) % names.len()];
        let id = |offset: usize| {
            if is_name(name(offset)) {
                v5(name(offset))
            } else {
                Uuid::from_u128(0x1000 + offset as u128)
            }
        };
        let node = |id: Uuid, function: Uuid, bind: Vec<(Uuid, Source)>| GraphNode {
            id,
            function,
            bind: bind.into_iter().collect(),
            children: None,
        };
        let key = Source::Key(key.to_string());
        let graph = Graph {
            root: Some(id(0)),
            nodes: vec![
                node(id(0), shadowed, vec![(shadowed_p, key.clone())]),
                node(
                    id(1),
                    spaced,
                    vec![
                        (spaced_p, Source::Port(Port::new(id(2), _RET_PARAM_ID))),
                        (_RET_PARAM_ID, key),
                    ],
                ),
                node(
                    id(2),
                    twin,
                    vec![(twin_p, Source::Value(Value::F64(1e300)))],
                ),
            ],
        };
        let tree = build_behavior_tree(&graph, &|_| None).expect("the tree builds");
        let node_names: HashMap<Uuid, String> = (0..3)
            .map(|offset| (id(offset), name(offset).to_string()))
            .collect();
        let shown = tree
            .display(Names {
                functions: Some(&functions),
                variables: None,
                nodes: Some(&node_names),
            })
            .to_string();
        let parsed = parse(&shown);
        assert_eq!(parsed.diagnostics, [], "{shown}");
        let read = resolve(&parsed.document, &functions);
        assert_eq!(
            errors(&read.diagnostics),
            Vec::<&Diagnostic>::new(),
            "{shown}"
        );
        assert_eq!(sorted(read.graph), sorted(graph), "{shown}");
    }
}

/// Dotted parameter names, as `@arora/dataflow` declares its nodes'
/// parameters (`param.half_life`), resolve, read through ports, and show and
/// read back; a module function's unstated parameters are not warned about.
#[test]
fn a_dataflow_node_reads_back() {
    let functions = arora_dataflow_nodes::functions::function_index();
    let text = "\
below(x: 1, limit: @d.state[0])

d: {face/out} = damp(in: {face/x}, param.half_life: 0.12f32, t: {arora/time}, state: ())
s: spring(in: @d._ret, param.stiffness: 120f32, param.damping: 14f32, t: {arora/time}, state: ())
";
    let mut index = index();
    index.extend(
        functions
            .iter()
            .map(|(id, function)| (*id, function.clone())),
    );
    let loaded = resolve(&parsed(text), &index);
    assert_eq!(loaded.diagnostics, [], "{:?}", loaded.diagnostics);
    let damp = loaded.graph.node(&v5("d")).unwrap();
    let half_life = parameter_named(&index[&damp.function], "param.half_life");
    assert_eq!(
        *link(&loaded.graph, v5("d"), half_life),
        Source::Value(Value::F32(0.12))
    );

    let tree = build_behavior_tree(&loaded.graph, &|_| None).unwrap();
    let node_names: HashMap<Uuid, String> = loaded
        .nodes
        .iter()
        .filter_map(|node| Some((node.id, node.name.clone()?)))
        .collect();
    let shown = tree
        .display(Names {
            functions: Some(&index),
            variables: None,
            nodes: Some(&node_names),
        })
        .to_string();
    assert!(shown.contains("param.half_life: 0.12f32"), "{shown}");
    let read = resolve(&parsed(&shown), &index);
    assert_eq!(
        errors(&read.diagnostics),
        Vec::<&Diagnostic>::new(),
        "{shown}"
    );
    assert_eq!(sorted(read.graph), sorted(loaded.graph), "{shown}");

    let port = parse("below(x: @d.param.half_life, limit: 1)\n");
    let ExprKind::Port { parameter, .. } = &port.document.nodes[0].call.arguments[0].value.kind
    else {
        panic!("a port");
    };
    assert_eq!(parameter.text, "param.half_life");
}

/// The parameter of `function` named `name`.
fn parameter_named(function: &arora_behavior::ModuleFunction, name: &str) -> Uuid {
    *function
        .function
        .parameters
        .iter()
        .find(|(_, parameter)| parameter.name == name)
        .unwrap_or_else(|| panic!("a parameter {name}"))
        .0
}
