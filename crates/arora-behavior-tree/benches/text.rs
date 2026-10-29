//! What reading a behavior text costs: parsing it, and resolving it against a
//! function index, on a generated text of a given size.
//!
//! `cargo bench -p arora-behavior-tree --bench text -- <groups> <iterations> <mode>`
//! generates a text of `<groups>` groups, each a condition in the tree and
//! three detached nodes reading one another and the store, a call in an
//! argument among them, and reads it `<iterations>` times. `<mode>` is
//! `parse` (parsing alone) or `resolve` (parsing, then resolving).
//!
//! Count instructions, not time: run it under callgrind, or difference the
//! process's instructions retired (`/usr/bin/time -l` on macOS) between
//! `<iterations>` and 0 iterations, and divide by the text's bytes, which it
//! prints. The cost of resolving is a `resolve` iteration less a `parse` one.
//! The time it prints is indicative only.

use std::collections::HashMap;
use std::fmt::Write;
use std::time::Instant;

use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_behavior_tree::text::{parse, resolve, Severity};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use uuid::Uuid;

/// `mul(a, b)`, `add(a, b)`, `clamp(x, lo, hi?)`, `below(x, limit)`, all of
/// `f64`s.
fn function_index() -> FunctionIndex {
    let f64 = || FrozenTy::from(PrimitiveKind::F64);
    let mut next = 0u128;
    let mut id = || {
        next += 1;
        Uuid::from_u128(next)
    };
    let mut index = HashMap::new();
    for (name, parameters) in [
        ("mul", &["a", "b"][..]),
        ("add", &["a", "b"]),
        ("clamp", &["x", "lo", "hi"]),
        ("below", &["x", "limit"]),
    ] {
        let function_id = id();
        let parameters: Vec<(Uuid, Parameter)> = parameters
            .iter()
            .map(|name| {
                let ty = match *name {
                    "hi" => FrozenTy::FrozenOption(FrozenOption {
                        element: Box::new(f64()),
                    }),
                    _ => f64(),
                };
                (
                    id(),
                    Parameter {
                        name: name.to_string(),
                        ty,
                        mutable: false,
                    },
                )
            })
            .collect();
        index.insert(
            function_id,
            ModuleFunction {
                module_id: Uuid::from_u128(0xF000),
                function_id,
                function_name: name.to_string(),
                function: Function {
                    parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
                    parameters: parameters.into_iter().collect(),
                    return_ty: f64(),
                },
            },
        );
    }
    index
}

/// A tree of `groups` conditions under a sequence, each reading the last of
/// its group's detached nodes.
fn text(groups: usize) -> String {
    let mut text = String::from("parallel\n  sequence\n");
    for group in 0..groups {
        let _ = writeln!(text, "    below(x: @level_{group}._ret, limit: 30)");
    }
    text.push('\n');
    for group in 0..groups {
        let _ = writeln!(
            text,
            "raw_{group}: mul(a: {{sensor/distance_{group}}}, b: 100)\n\
             cm_{group}: clamp(x: @raw_{group}._ret, lo: 0, hi: 500.5)\n\
             level_{group}: {{face/level_{group}}} = add(a: @cm_{group}._ret, b: mul(a: {{face/gain}}, b: 1.5f64))"
        );
    }
    text
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let groups: usize = args.first().map_or(1_000, |n| n.parse().expect("<groups>"));
    let iterations: u32 = args.get(1).map_or(20, |n| n.parse().expect("<iterations>"));
    let mode = args.get(2).map_or("resolve", String::as_str);

    let functions = function_index();
    let text = text(groups);
    let read = || {
        let parsed = parse(&text);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        if mode == "resolve" {
            let loaded = resolve(&parsed.document, &functions);
            assert!(
                loaded
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity != Severity::Error),
                "{:?}",
                loaded.diagnostics
            );
            // The parallel, the sequence, and per group a condition, three
            // detached nodes and the call in an argument.
            assert_eq!(loaded.graph.nodes.len(), 2 + 5 * groups);
        }
    };
    read();
    let start = Instant::now();
    for _ in 0..iterations {
        read();
    }
    let elapsed = start.elapsed();
    println!(
        "{mode} {groups} groups, {} bytes: {iterations} iterations, {:.1} ns/byte (indicative)",
        text.len(),
        elapsed.as_nanos() as f64 / f64::from(iterations.max(1)) / text.len() as f64
    );
}
